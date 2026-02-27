//! # Hysteria2 QUIC Client
//!
//! Connects to the Hysteria2 server via QUIC/HTTP3 and exposes a local
//! SOCKS5 proxy.  Each accepted SOCKS5 connection opens a new QUIC
//! bidirectional stream to the server.
//!
//! ## Usage
//!
//! ```sh
//! vpn-client connect --server HOST --transport hysteria --mode socks5
//! ```

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use http::Request;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{debug, info, warn};
use vpn_common::hysteria::{auth_headers, HysteriaAddr, TcpRequest, TcpResponse, AUTH_SUCCESS_STATUS};

// ─── TLS: skip-verify implementation ──────────────────────────────────────

/// Skip all TLS certificate checks (use when `--tls-skip-verify`).
///
/// Traffic is still TLS-encrypted; only the identity of the server is not
/// verified.  Use `--tls-fingerprint` in production for proper pinning.
#[derive(Debug)]
struct SkipVerify;

impl rustls::client::danger::ServerCertVerifier for SkipVerify {
    fn verify_server_cert(
        &self, _ee: &rustls::pki_types::CertificateDer,
        _inters: &[rustls::pki_types::CertificateDer],
        _sn: &rustls::pki_types::ServerName,
        _ocsp: &[u8], _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self, _msg: &[u8], _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self, _msg: &[u8], _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

// ─── Public entry point ────────────────────────────────────────────────────

/// Connect to a Hysteria2 server and expose a local SOCKS5 proxy.
pub async fn run_hysteria_socks5(
    server_host: &str,
    hysteria_port: u16,
    password: &str,
    socks_port: u16,
    tls_skip_verify: bool,
) -> Result<()> {
    info!(server=%server_host, port=hysteria_port, "Connecting via Hysteria2");

    let endpoint = build_quic_client(tls_skip_verify)?;

    // Resolve hostname to socket address
    let addr: SocketAddr = tokio::net::lookup_host(format!("{server_host}:{hysteria_port}"))
        .await.context("DNS lookup failed")?
        .next().context("no DNS result")?;

    let conn = endpoint
        .connect(addr, server_host).context("QUIC connect init")?
        .await.context("QUIC handshake failed")?;

    info!("QUIC connected to {addr}");

    // Authenticate via HTTP/3
    authenticate_h3(&conn, password).await.context("Hysteria2 auth failed")?;

    info!("Hysteria2 authenticated — SOCKS5 on 127.0.0.1:{socks_port}");

    let conn = Arc::new(conn);

    // Start SOCKS5 listener
    let listener = TcpListener::bind(format!("127.0.0.1:{socks_port}"))
        .await.with_context(|| format!("SOCKS5 bind failed on port {socks_port}"))?;

    println!("\x1b[32m✓ Hysteria2 VPN aktiven\x1b[0m");
    println!("  SOCKS5: 127.0.0.1:{socks_port}");
    println!("  Сервер: {server_host}:{hysteria_port}");
    println!("  Нажмите Ctrl-C для отключения.\n");

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => { warn!("SOCKS5 accept: {e}"); continue; }
        };
        let c = conn.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_socks5(stream, c).await {
                debug!(%peer, "SOCKS5 session: {e:#}");
            }
        });
    }
}

// ─── QUIC endpoint ─────────────────────────────────────────────────────────

fn build_quic_client(tls_skip_verify: bool) -> Result<quinn::Endpoint> {
    let mut tls = if tls_skip_verify {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipVerify))
            .with_no_client_auth()
    } else {
        let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };
    tls.alpn_protocols = vec![b"h3".to_vec()];

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into()?));
    transport.datagram_receive_buffer_size(Some(4 * 1024 * 1024));
    transport.datagram_send_buffer_size(4 * 1024 * 1024);
    transport.keep_alive_interval(Some(Duration::from_secs(10)));

    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .context("quinn: client crypto build failed")?;
    let mut cfg = quinn::ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(Arc::new(transport));

    let mut ep = quinn::Endpoint::client("0.0.0.0:0".parse()?)
        .context("quinn: client bind failed")?;
    ep.set_default_client_config(cfg);
    Ok(ep)
}

// ─── HTTP/3 authentication ─────────────────────────────────────────────────

/// Authenticate with the server via HTTP/3 POST /auth.
async fn authenticate_h3(conn: &quinn::Connection, password: &str) -> Result<()> {
    let h3_conn = h3_quinn::Connection::new(conn.clone());
    let (mut driver, mut send_request) = h3::client::new(h3_conn)
        .await.context("H3 client init")?;

    // Drive H3 connection in background
    tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let req = Request::builder()
        .method("POST")
        .uri("/auth")
        .header(auth_headers::AUTH, password)
        .header(auth_headers::CC_RX, "0")
        .header(auth_headers::PADDING, "00")
        .body(()).context("build auth request")?;

    let mut stream = send_request.send_request(req).await.context("H3 send_request")?;
    stream.finish().await.context("H3 stream finish")?;

    let resp = stream.recv_response().await.context("H3 recv_response")?;
    let status = resp.status().as_u16();
    if status != AUTH_SUCCESS_STATUS {
        bail!("Hysteria2 auth rejected (HTTP {status}) — wrong password?");
    }
    Ok(())
}

// ─── SOCKS5 handler ────────────────────────────────────────────────────────

/// Handle one SOCKS5 connection: parse CONNECT target → open QUIC stream → relay.
async fn handle_socks5(mut client: TcpStream, quic: Arc<quinn::Connection>) -> Result<()> {
    // --- SOCKS5 greeting ---
    let mut hdr = [0u8; 2];
    client.read_exact(&mut hdr).await?;
    if hdr[0] != 5 { bail!("not SOCKS5"); }
    let n = hdr[1] as usize;
    let mut methods = vec![0u8; n];
    client.read_exact(&mut methods).await?;
    client.write_all(&[5u8, 0u8]).await?; // no-auth

    // --- CONNECT request ---
    let mut req = [0u8; 4];
    client.read_exact(&mut req).await?;
    if req[1] != 1 { // only CONNECT
        client.write_all(&[5u8, 7u8, 0u8, 1u8, 0u8,0u8,0u8,0u8, 0u8,0u8]).await.ok();
        bail!("unsupported SOCKS5 cmd {}", req[1]);
    }

    let addr = match req[3] {
        0x01 => { // IPv4
            let mut ip = [0u8; 4]; client.read_exact(&mut ip).await?;
            let mut p = [0u8; 2]; client.read_exact(&mut p).await?;
            HysteriaAddr::V4(format!("{}.{}.{}.{}:{}", ip[0],ip[1],ip[2],ip[3], u16::from_be_bytes(p)).parse()?)
        }
        0x03 => { // Hostname
            let mut lb = [0u8; 1]; client.read_exact(&mut lb).await?;
            let mut host = vec![0u8; lb[0] as usize]; client.read_exact(&mut host).await?;
            let mut p = [0u8; 2]; client.read_exact(&mut p).await?;
            HysteriaAddr::Name(String::from_utf8(host)?, u16::from_be_bytes(p))
        }
        0x04 => { // IPv6
            let mut ip = [0u8; 16]; client.read_exact(&mut ip).await?;
            let mut p = [0u8; 2]; client.read_exact(&mut p).await?;
            HysteriaAddr::V6(format!("[{}]:{}", std::net::Ipv6Addr::from(ip), u16::from_be_bytes(p)).parse()?)
        }
        t => bail!("unknown addr type {t:#x}"),
    };

    // --- Open QUIC stream ---
    let (mut qs, mut qr) = quic.open_bi().await.context("QUIC open_bi")?;
    qs.write_all(&TcpRequest { addr }.encode()).await.context("TcpRequest write")?;

    // --- Read TcpResponse ---
    let mut rbuf = vec![0u8; 256];
    let n = qr.read(&mut rbuf).await?.unwrap_or(0);
    let tr = TcpResponse::decode(Bytes::copy_from_slice(&rbuf[..n]))?;
    if !tr.ok {
        client.write_all(&[5u8,4u8,0u8,1u8, 0u8,0u8,0u8,0u8, 0u8,0u8]).await.ok();
        bail!("server error: {}", tr.message);
    }

    // --- Tell SOCKS5 client: success ---
    client.write_all(&[5u8,0u8,0u8,1u8, 0u8,0u8,0u8,0u8, 0u8,0u8]).await?;

    // --- Bidirectional relay ---
    let (mut cr, mut cw) = client.into_split();
    let c2q = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match cr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => { if qs.write_all(&buf[..n]).await.is_err() { break; } }
            }
        }
        qs.finish().ok();
    });
    let q2c = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match qr.read(&mut buf).await {
                Ok(Some(n @ 1..)) => { if cw.write_all(&buf[..n]).await.is_err() { break; } }
                _ => break,
            }
        }
    });
    let _ = tokio::join!(c2q, q2c);
    Ok(())
}
