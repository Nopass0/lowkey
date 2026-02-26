use std::sync::Arc;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use tokio::{signal, sync::Mutex};
use tracing::info;
use vpn_common::{from_hex, to_hex, VpnCrypto, DEFAULT_API_PORT};
use x25519_dalek::{PublicKey, StaticSecret};

#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── Session ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct Session {
    token: Option<String>,
    server: Option<String>,
    api_port: Option<u16>,
}

fn session_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".config").join("lowkey").join("session.json")
}

fn load_session() -> Session {
    std::fs::read_to_string(session_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_session(session: &Session) -> Result<()> {
    let path = session_path();
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(&path, serde_json::to_string_pretty(session)?)?;
    Ok(())
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Clone, ValueEnum, Debug, PartialEq)]
enum Mode { Tun, Socks5 }

#[derive(Parser)]
#[command(name = "vpn-client", about = "Lowkey VPN Client")]
struct Args {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Auth { #[command(subcommand)] sub: AuthCmd },
    Subscription { #[command(subcommand)] sub: SubCmd },
    Promo {
        #[arg(short, long)] server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
        #[arg(short, long)] code: String,
    },
    Connect {
        #[arg(short, long)] server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
        #[arg(long, default_value = "tun")] mode: Mode,
        #[arg(long)] udp_port: Option<u16>,
        #[arg(long)] proxy_port: Option<u16>,
        #[arg(long, default_value_t = 1080)] socks_port: u16,
        #[arg(long, default_value_t = false)] split_tunnel: bool,
    },
}

#[derive(Subcommand)]
enum AuthCmd {
    Register {
        #[arg(short, long)] server: String,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
        #[arg(short, long)] login: String,
        #[arg(short, long)] password: String,
    },
    Login {
        #[arg(short, long)] server: String,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
        #[arg(short, long)] login: String,
        #[arg(short, long)] password: String,
    },
    Me {
        #[arg(short, long)] server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
    },
    Logout,
}

#[derive(Subcommand)]
enum SubCmd {
    Plans {
        #[arg(short, long)] server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
    },
    Buy {
        #[arg(short, long)] server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
        #[arg(long, default_value = "standard")] plan: String,
    },
    Status {
        #[arg(short, long)] server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)] api_port: u16,
    },
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("vpn_client=info".parse()?))
        .init();

    match Args::parse().command {
        Cmd::Auth { sub } => handle_auth(sub).await?,
        Cmd::Subscription { sub } => handle_sub(sub).await?,
        Cmd::Promo { server, api_port, code } => {
            let session = load_session();
            let srv = server.or(session.server.clone()).context("--server required")?;
            let tok = session.token.context("Not logged in")?;
            let resp = api_post(&srv, api_port, "/promo/apply", &tok,
                &serde_json::json!({ "code": code })).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Cmd::Connect { server, api_port, mode, udp_port, proxy_port, socks_port, split_tunnel } => {
            let session = load_session();
            let srv = server.or(session.server.clone()).context("--server required")?;
            let tok = session.token.context("Not logged in. Run: vpn-client auth login")?;
            connect(&srv, api_port, udp_port, proxy_port, &tok, mode, socks_port, split_tunnel).await?;
        }
    }
    Ok(())
}

// ── Auth ──────────────────────────────────────────────────────────────────────

async fn handle_auth(cmd: AuthCmd) -> Result<()> {
    match cmd {
        AuthCmd::Register { server, api_port, login, password } => {
            let resp = api_anon(&server, api_port, "/auth/register",
                &serde_json::json!({ "login": login, "password": password })).await?;
            let tok = resp["token"].as_str().unwrap_or("").to_string();
            println!("Registered as '{}'\n{}", login, serde_json::to_string_pretty(&resp["user"])?);
            save_session(&Session { token: Some(tok), server: Some(server), api_port: Some(api_port) })?;
        }
        AuthCmd::Login { server, api_port, login, password } => {
            let resp = api_anon(&server, api_port, "/auth/login",
                &serde_json::json!({ "login": login, "password": password })).await?;
            let tok = resp["token"].as_str().context("No token")?.to_string();
            println!("Logged in as '{}'\n{}", login, serde_json::to_string_pretty(&resp["user"])?);
            save_session(&Session { token: Some(tok), server: Some(server), api_port: Some(api_port) })?;
        }
        AuthCmd::Me { server, api_port } => {
            let s = load_session();
            let srv = server.or(s.server).context("--server required")?;
            let tok = s.token.context("Not logged in")?;
            println!("{}", serde_json::to_string_pretty(&api_get(&srv, api_port, "/auth/me", &tok).await?)?);
        }
        AuthCmd::Logout => { save_session(&Session::default())?; println!("Logged out."); }
    }
    Ok(())
}

// ── Subscription ──────────────────────────────────────────────────────────────

async fn handle_sub(cmd: SubCmd) -> Result<()> {
    match cmd {
        SubCmd::Plans { server, api_port } => {
            let s = load_session();
            let srv = server.or(s.server).context("--server required")?;
            let resp: serde_json::Value = reqwest::Client::new()
                .get(format!("http://{}:{}/subscription/plans", srv, api_port))
                .send().await?.json().await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        SubCmd::Buy { server, api_port, plan } => {
            let s = load_session();
            let srv = server.or(s.server.clone()).context("--server required")?;
            let tok = s.token.context("Not logged in")?;
            let resp = api_post(&srv, api_port, "/subscription/buy", &tok,
                &serde_json::json!({ "plan_id": plan })).await?;
            println!("Subscription activated!\n{}", serde_json::to_string_pretty(&resp)?);
        }
        SubCmd::Status { server, api_port } => {
            let s = load_session();
            let srv = server.or(s.server).context("--server required")?;
            let tok = s.token.context("Not logged in")?;
            println!("{}", serde_json::to_string_pretty(
                &api_get(&srv, api_port, "/subscription/status", &tok).await?)?);
        }
    }
    Ok(())
}

// ── Connect ───────────────────────────────────────────────────────────────────

async fn connect(server: &str, api_port: u16, udp_override: Option<u16>,
    proxy_override: Option<u16>, token: &str, mode: Mode, socks_port: u16, split: bool,
) -> Result<()> {
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);

    let reg = api_post(server, api_port, "/api/peers/register", token, &serde_json::json!({
        "public_key": to_hex(public.as_bytes()), "psk": ""
    })).await.context("Registration failed — check subscription")?;

    let vpn_ip: std::net::Ipv4Addr = reg["assigned_ip"].as_str().context("No assigned_ip")?.parse()?;
    let udp_port  = udp_override.or_else(|| reg["udp_port"].as_u64().map(|p| p as u16)).unwrap_or(51820);
    let proxy_port = proxy_override.or_else(|| reg["proxy_port"].as_u64().map(|p| p as u16)).unwrap_or(8388);

    let spub: Vec<u8> = from_hex(reg["server_public_key"].as_str().unwrap_or(""))
        .filter(|b| b.len() == 32).context("Bad server pubkey")?;
    let mut spub_arr = [0u8; 32]; spub_arr.copy_from_slice(&spub);
    let shared = secret.diffie_hellman(&PublicKey::from(spub_arr));
    let crypto = Arc::new(VpnCrypto::from_shared_secret(&shared));

    info!("VPN IP: {}  mode: {:?}", vpn_ip, mode);

    match mode {
        Mode::Tun => {
            #[cfg(unix)] { run_tun_mode(server, vpn_ip, udp_port, crypto, split).await?; }
            #[cfg(not(unix))] { anyhow::bail!("TUN requires Linux/macOS. Use --mode socks5"); }
        }
        Mode::Socks5 => run_socks5_mode(server, proxy_port, socks_port, token, &secret).await?,
    }
    Ok(())
}

// ── TUN mode (Unix) ───────────────────────────────────────────────────────────

#[cfg(unix)]
async fn run_tun_mode(server: &str, vpn_ip: std::net::Ipv4Addr,
    udp_port: u16, crypto: Arc<VpnCrypto>, split: bool,
) -> Result<()> {
    use tokio::net::UdpSocket;
    let mut cfg = tun::Configuration::default();
    cfg.address(vpn_ip.to_string().as_str())
       .netmask(vpn_common::VPN_NETMASK).destination("10.0.0.1").up();
    cfg.platform(|c| { c.packet_information(false); });
    let dev = tun::create_as_async(&cfg).context("TUN failed — run as root")?;
    info!("TUN up ({})", vpn_ip);
    let orig_gw = get_gw()?;
    setup_routing(server, &orig_gw, split)?;
    info!("Routing active. Ctrl-C to disconnect.");

    let udp = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let srv: std::net::SocketAddr = format!("{server}:{udp_port}").parse()?;
    let ib = vpn_ip.octets();
    { let enc = crypto.encrypt(b"hello"); let mut p = Vec::new(); p.extend_from_slice(&ib); p.extend_from_slice(&enc); udp.send_to(&p, srv).await?; }

    let (rx, tx) = tokio::io::split(dev);
    let tx = Arc::new(Mutex::new(tx));
    { let (u,c) = (udp.clone(), crypto.clone()); tokio::spawn(async move { let _ = t2u(rx, u, c, ib, srv).await; }); }
    { let (u,c,tw) = (udp.clone(), crypto.clone(), tx.clone()); tokio::spawn(async move { let _ = u2t(u, tw, c).await; }); }
    signal::ctrl_c().await?;
    restore_routing(server, &orig_gw, split);
    Ok(())
}

#[cfg(unix)]
async fn t2u(mut tun: impl AsyncReadExt + Unpin, udp: Arc<tokio::net::UdpSocket>,
    crypto: Arc<VpnCrypto>, ib: [u8; 4], srv: std::net::SocketAddr,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let n = tun.read(&mut buf).await?; if n == 0 { break; }
        let enc = crypto.encrypt(&buf[..n]);
        let mut p = Vec::new(); p.extend_from_slice(&ib); p.extend_from_slice(&enc);
        let _ = udp.send_to(&p, srv).await;
    }
    Ok(())
}

#[cfg(unix)]
async fn u2t(udp: Arc<tokio::net::UdpSocket>,
    tun: Arc<Mutex<impl AsyncWriteExt + Unpin>>, crypto: Arc<VpnCrypto>,
) -> Result<()> {
    let mut buf = vec![0u8; 65600];
    loop {
        let (n, _) = udp.recv_from(&mut buf).await?;
        if let Some(plain) = crypto.decrypt(&buf[..n]) { let _ = tun.lock().await.write_all(&plain).await; }
    }
}

// ── SOCKS5 mode ───────────────────────────────────────────────────────────────

async fn run_socks5_mode(server: &str, proxy_port: u16, socks_port: u16,
    token: &str, my_secret: &StaticSecret,
) -> Result<()> {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind(format!("127.0.0.1:{socks_port}"))
        .await.with_context(|| format!("Cannot bind :{socks_port}"))?;
    println!("SOCKS5 proxy on 127.0.0.1:{socks_port}\nSet system proxy → SOCKS5 127.0.0.1:{socks_port}\nCtrl-C to disconnect.");

    let sb = my_secret.to_bytes();
    let sa = format!("{server}:{proxy_port}");
    let tok = token.to_string();
    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);
    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            res = listener.accept() => {
                let (stream, _) = res?;
                let sa = sa.clone(); let sb = sb; let tok = tok.clone();
                tokio::spawn(async move {
                    if let Err(e) = socks5_conn(stream, &sa, &sb, &tok).await {
                        tracing::trace!("socks5: {e}");
                    }
                });
            }
        }
    }
    println!("Disconnected.");
    Ok(())
}

async fn socks5_conn(mut cl: tokio::net::TcpStream, vpn_addr: &str,
    sb: &[u8; 32], psk: &str,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 512];

    cl.read_exact(&mut buf[..2]).await?;
    if buf[0] != 5 { anyhow::bail!("Not SOCKS5"); }
    let nm = buf[1] as usize;
    cl.read_exact(&mut buf[..nm]).await?;
    cl.write_all(&[5, 0]).await?;

    cl.read_exact(&mut buf[..4]).await?;
    if buf[1] != 1 { cl.write_all(&[5,7,0,1,0,0,0,0,0,0]).await?; anyhow::bail!("Only CONNECT"); }

    let (addr_bytes, port): (Vec<u8>, u16) = match buf[3] {
        1 => { cl.read_exact(&mut buf[..6]).await?; let mut a = vec![1]; a.extend_from_slice(&buf[..4]); (a, u16::from_be_bytes([buf[4],buf[5]])) }
        3 => { cl.read_exact(&mut buf[..1]).await?; let hl = buf[0] as usize; cl.read_exact(&mut buf[..hl+2]).await?; let mut a = vec![3, hl as u8]; a.extend_from_slice(&buf[..hl]); (a, u16::from_be_bytes([buf[hl],buf[hl+1]])) }
        4 => { cl.read_exact(&mut buf[..18]).await?; let mut a = vec![4]; a.extend_from_slice(&buf[..16]); (a, u16::from_be_bytes([buf[16],buf[17]])) }
        _ => anyhow::bail!("Unknown atype"),
    };

    let my_secret = StaticSecret::from(*sb);
    let my_pub = PublicKey::from(&my_secret);
    let mut vs = tokio::net::TcpStream::connect(vpn_addr).await.context("VPN connect failed")?;
    vs.write_all(my_pub.as_bytes()).await?;
    let mut spb = [0u8; 32]; vs.read_exact(&mut spb).await?;
    let fc = vpn_common::FramedCrypto::new(&my_secret, &PublicKey::from(spb));

    let auth = vpn_common::psk_auth_token(psk);
    let mut hdr = Vec::new(); hdr.extend_from_slice(&auth); hdr.extend_from_slice(&addr_bytes); hdr.extend_from_slice(&port.to_be_bytes());
    vs.write_all(&fc.encode(&hdr)).await?;

    let status = recv_frame(&mut vs, &fc).await?;
    if status.first() != Some(&0) { cl.write_all(&[5,5,0,1,0,0,0,0,0,0]).await?; anyhow::bail!("Proxy rejected"); }
    cl.write_all(&[5,0,0,1,0,0,0,0,0,0]).await?;

    let fc = Arc::new(fc);
    let (mut crx, mut ctx) = cl.into_split();
    let (mut vrx, mut vtx) = vs.into_split();
    let fc1 = fc.clone();
    let t1 = tokio::spawn(async move {
        let mut tmp = vec![0u8; 65535];
        loop { let n = match crx.read(&mut tmp).await { Ok(0)|Err(_)=>break, Ok(n)=>n }; if vtx.write_all(&fc1.encode(&tmp[..n])).await.is_err() { break; } }
    });
    let t2 = tokio::spawn(async move {
        let mut fb = Vec::<u8>::new(); let mut tmp = vec![0u8; 65536];
        loop {
            let n = match vrx.read(&mut tmp).await { Ok(0)|Err(_)=>break, Ok(n)=>n };
            fb.extend_from_slice(&tmp[..n]);
            loop { match fc.decode(&fb) { Some((p,c)) => { if ctx.write_all(&p).await.is_err(){return;} fb.drain(..c); } None=>break } }
        }
    });
    let _ = tokio::join!(t1, t2);
    Ok(())
}

async fn recv_frame(s: &mut tokio::net::TcpStream, fc: &vpn_common::FramedCrypto) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut lb = [0u8;2]; s.read_exact(&mut lb).await?;
    let cl = u16::from_be_bytes(lb) as usize;
    let mut raw = vec![0u8; 12+cl]; s.read_exact(&mut raw).await?;
    let mut buf = Vec::new(); buf.extend_from_slice(&lb); buf.extend_from_slice(&raw);
    fc.decode(&buf).map(|(p,_)|p).context("Frame decrypt failed")
}

// ── Routing ───────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn get_gw() -> Result<String> {
    let o = std::process::Command::new("sh").args(["-c","ip route show default|awk '/default/{print $3;exit}'"]).output()?;
    let g = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if g.is_empty() { anyhow::bail!("No default gateway"); }
    Ok(g)
}
#[cfg(unix)]
fn setup_routing(server: &str, gw: &str, split: bool) -> Result<()> {
    use std::process::Command;
    if split { Command::new("ip").args(["route","add","10.0.0.0/24","dev","tun0"]).output()?; return Ok(()); }
    let _ = Command::new("ip").args(["route","del",&format!("{server}/32")]).output();
    Command::new("ip").args(["route","add",&format!("{server}/32"),"via",gw]).output()?;
    Command::new("ip").args(["route","replace","default","via","10.0.0.1","dev","tun0"]).output()?;
    Ok(())
}
#[cfg(unix)]
fn restore_routing(server: &str, gw: &str, split: bool) {
    use std::process::Command;
    if split { let _ = Command::new("ip").args(["route","del","10.0.0.0/24","dev","tun0"]).output(); return; }
    let _ = Command::new("ip").args(["route","del",&format!("{server}/32")]).output();
    let _ = Command::new("ip").args(["route","replace","default","via",gw]).output();
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

async fn api_anon(server: &str, port: u16, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    Ok(reqwest::Client::new().post(format!("http://{}:{}{}", server, port, path))
        .json(body).send().await?.error_for_status()?.json().await?)
}
async fn api_post(server: &str, port: u16, path: &str, tok: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    Ok(reqwest::Client::new().post(format!("http://{}:{}{}", server, port, path))
        .bearer_auth(tok).json(body).send().await?.error_for_status()?.json().await?)
}
async fn api_get(server: &str, port: u16, path: &str, tok: &str) -> Result<serde_json::Value> {
    Ok(reqwest::Client::new().get(format!("http://{}:{}{}", server, port, path))
        .bearer_auth(tok).send().await?.error_for_status()?.json().await?)
}
