//! # Hysteria2 QUIC Server
//!
//! Hysteria2-compatible QUIC/HTTP3 server that allows clients to use the
//! `--transport hysteria` flag or the official Hysteria2 client.
//!
//! ## Protocol
//!
//! 1. QUIC connection established over TLS 1.3 (ALPN: "h3").
//! 2. Client sends `POST /auth HTTP/3` with `Hysteria-Auth: <password>`.
//! 3. Server replies HTTP **233** on success, **403** otherwise.
//! 4. Same QUIC connection used for:
//!    - **TCP proxy**: bidirectional QUIC streams (TcpRequest → TcpResponse → data)
//!    - **UDP relay**: QUIC unreliable datagrams (UdpMessage format)
//!
//! ## TLS
//!
//! A fresh self-signed certificate is generated on each startup via `rcgen`.
//! The SHA-256 fingerprint is printed so operators can pin it in clients.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use bytes::Bytes;
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Mutex,
};
use tracing::{debug, info, warn};
use vpn_common::hysteria::{
    auth_headers, HysteriaAddr, TcpRequest, TcpResponse, UdpMessage, AUTH_SUCCESS_STATUS,
};

use crate::state::ServerState;

// ─── Configuration ─────────────────────────────────────────────────────────

/// Runtime configuration for the Hysteria2 QUIC server.
#[derive(Clone)]
pub struct HysteriaConfig {
    /// UDP port the QUIC endpoint listens on (default: 8443).
    pub port: u16,
    /// Password clients must present in the `Hysteria-Auth` header.
    pub password: String,
    /// Optional Salamander obfuscation password (`None` = plain QUIC).
    pub obfs_password: Option<String>,
    /// Server max upload rate advertised to clients (0 = unlimited).
    pub max_download_mbps: u64,
    /// Server max download rate advertised to clients (0 = unlimited).
    pub max_upload_mbps: u64,
}

impl HysteriaConfig {
    /// Build config from environment variables, falling back to `psk`.
    pub fn from_env(psk: &str) -> Self {
        HysteriaConfig {
            port: std::env::var("HYSTERIA_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8443),
            password: std::env::var("HYSTERIA_PASSWORD").unwrap_or_else(|_| psk.to_string()),
            obfs_password: std::env::var("HYSTERIA_OBFS").ok(),
            max_download_mbps: 0,
            max_upload_mbps: 0,
        }
    }
}

// ─── TLS helpers ───────────────────────────────────────────────────────────

/// Generate a self-signed TLS certificate for the QUIC endpoint.
///
/// Returns `(cert_chain, private_key)` in DER format.
pub fn generate_self_signed_cert() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert = generate_simple_self_signed(vec!["lowkey.local".to_string()])
        .context("rcgen: failed to generate certificate")?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::try_from(cert.key_pair.serialize_der())
        .map_err(|e| anyhow::anyhow!("key conversion: {e}"))?;
    Ok((vec![cert_der], key_der))
}

/// Log the SHA-256 fingerprint of `certs[0]` so operators can pin it.
pub fn log_cert_fingerprint(certs: &[CertificateDer]) {
    if let Some(cert) = certs.first() {
        use sha2::{Digest, Sha256};
        let fp: String = Sha256::digest(cert.as_ref())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");
        info!(fingerprint = %fp, "Hysteria2 TLS fingerprint (pin this in clients with --tls-fingerprint)");
    }
}

// ─── QUIC endpoint ─────────────────────────────────────────────────────────

/// Build a Quinn QUIC server endpoint configured for Hysteria2.
pub fn build_quic_endpoint(
    port: u16,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::Endpoint> {
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("rustls: invalid cert/key")?;
    tls.alpn_protocols = vec![b"h3".to_vec()];

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into()?));
    transport.datagram_receive_buffer_size(Some(4 * 1024 * 1024));
    transport.datagram_send_buffer_size(4 * 1024 * 1024);

    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .context("quinn: server crypto build failed")?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    server_config.transport_config(Arc::new(transport));

    let addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    quinn::Endpoint::server(server_config, addr)
        .with_context(|| format!("quinn: bind failed on :{port}"))
}

// ─── Main server loop ──────────────────────────────────────────────────────

/// Start the Hysteria2 QUIC server.
pub async fn run_hysteria_server(config: HysteriaConfig, _state: Arc<ServerState>) -> Result<()> {
    let (certs, key) = generate_self_signed_cert()?;
    log_cert_fingerprint(&certs);

    let endpoint = build_quic_endpoint(config.port, certs, key)?;
    info!(port = config.port, "Hysteria2 QUIC server listening");

    while let Some(incoming) = endpoint.accept().await {
        let cfg = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(incoming, cfg).await {
                debug!("hysteria conn error: {e:#}");
            }
        });
    }
    Ok(())
}

// ─── Per-connection handler ────────────────────────────────────────────────

/// Handle a single QUIC connection: authenticate via H3, then relay.
async fn handle_connection(incoming: quinn::Incoming, config: HysteriaConfig) -> Result<()> {
    let conn = incoming.await.context("QUIC handshake failed")?;
    let peer = conn.remote_address();

    // HTTP/3 authentication
    let h3_conn = h3_quinn::Connection::new(conn.clone());
    let mut h3_server: h3::server::Connection<h3_quinn::Connection, bytes::Bytes> =
        h3::server::Connection::new(h3_conn).await.context("H3 init failed")?;

    let authenticated = authenticate_h3(&mut h3_server, &config, peer).await?;
    if !authenticated {
        debug!(%peer, "Hysteria2 auth failed");
        return Ok(());
    }
    info!(%peer, "Hysteria2 client authenticated");

    // Accept QUIC bidirectional streams for TCP proxying
    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_stream(send, recv).await {
                        debug!("TCP stream error: {e:#}");
                    }
                });
                // Also check for UDP datagrams in parallel
                let conn2 = conn.clone();
                tokio::spawn(async move {
                    relay_udp_datagrams(conn2).await;
                });
                // Only spawn one UDP relay task per connection
                break;
            }
            Err(quinn::ConnectionError::ApplicationClosed(_))
            | Err(quinn::ConnectionError::ConnectionClosed(_)) => {
                debug!(%peer, "connection closed");
                return Ok(());
            }
            Err(e) => {
                warn!(%peer, "accept_bi error: {e}");
                return Ok(());
            }
        }
    }

    // Accept remaining TCP streams
    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_stream(send, recv).await {
                        debug!("TCP stream error: {e:#}");
                    }
                });
            }
            Err(quinn::ConnectionError::ApplicationClosed(_))
            | Err(quinn::ConnectionError::ConnectionClosed(_)) => break,
            Err(e) => {
                warn!(%peer, "accept_bi error: {e}");
                break;
            }
        }
    }
    Ok(())
}

// ─── HTTP/3 authentication ─────────────────────────────────────────────────

/// Run the H3 auth handshake on the given connection.
///
/// Returns `true` if authentication succeeded.
async fn authenticate_h3(
    h3_server: &mut h3::server::Connection<h3_quinn::Connection, bytes::Bytes>,
    config: &HysteriaConfig,
    peer: SocketAddr,
) -> Result<bool> {
    // Wait up to 10 s for the auth request
    let resolver = match tokio::time::timeout(Duration::from_secs(10), h3_server.accept()).await {
        Ok(Ok(Some(r))) => r,
        Ok(Ok(None)) => return Ok(false),
        Ok(Err(e)) => { debug!(%peer, "H3 accept error: {e}"); return Ok(false); }
        Err(_)      => { debug!(%peer, "H3 auth timeout");     return Ok(false); }
    };

    // Resolve the full request (read headers)
    let (req, mut stream) = match resolver.resolve_request().await {
        Ok(v) => v,
        Err(e) => { debug!(%peer, "H3 resolve error: {e}"); return Ok(false); }
    };

    // Masquerade: non-/auth paths get a 403
    if req.uri().path() != "/auth" {
        let _ = stream.send_response(
            http::Response::builder().status(403).body(()).unwrap()
        ).await;
        let _ = stream.finish().await;
        return Ok(false);
    }

    let auth = req.headers()
        .get(auth_headers::AUTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if auth != config.password {
        debug!(%peer, "Hysteria2 bad password");
        let _ = stream.send_response(
            http::Response::builder().status(403).body(()).unwrap()
        ).await;
        let _ = stream.finish().await;
        return Ok(false);
    }

    // 233 OK — include bandwidth negotiation headers
    let tx_bps = config.max_upload_mbps * 1_000_000 / 8;
    let resp = http::Response::builder()
        .status(AUTH_SUCCESS_STATUS)
        .header(auth_headers::CC_TX, tx_bps.to_string())
        .header(auth_headers::UDP, "true")
        .body(())
        .unwrap();

    stream.send_response(resp).await.context("H3 send auth response")?;
    stream.finish().await.ok();
    Ok(true)
}

// ─── TCP Stream Proxy ──────────────────────────────────────────────────────

/// Relay a single Hysteria2 TCP proxy stream.
///
/// 1. Read TcpRequest (destination address).
/// 2. Connect to upstream.
/// 3. Send TcpResponse.
/// 4. Relay data bidirectionally.
async fn handle_tcp_stream(
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
) -> Result<()> {
    // Read TcpRequest (bounded: max 512 bytes is always enough for the header)
    let header = recv.read_to_end(512).await.context("TcpRequest read")?;
    let request = TcpRequest::decode(Bytes::from(header)).context("TcpRequest decode")?;
    let target = format!("{}:{}", request.addr.host(), request.addr.port());
    debug!(%target, "Hysteria2 TCP connect");

    match TcpStream::connect(&target).await {
        Err(e) => {
            let _ = send.write_all(&TcpResponse::error(e.to_string()).encode()).await;
            return Ok(());
        }
        Ok(upstream) => {
            send.write_all(&TcpResponse::success().encode())
                .await
                .context("TcpResponse write")?;

            let (mut tcp_r, mut tcp_w) = upstream.into_split();

            // upstream → QUIC
            let up = tokio::spawn(async move {
                let mut buf = [0u8; 65536];
                while let Ok(n @ 1..) = tcp_r.read(&mut buf).await {
                    if send.write_all(&buf[..n]).await.is_err() { break; }
                }
            });

            // QUIC → upstream
            let down = tokio::spawn(async move {
                let mut buf = [0u8; 65536];
                loop {
                    match recv.read(&mut buf).await {
                        Ok(Some(n @ 1..)) => {
                            if tcp_w.write_all(&buf[..n]).await.is_err() { break; }
                        }
                        _ => break,
                    }
                }
            });

            let _ = tokio::join!(up, down);
        }
    }
    Ok(())
}

// ─── UDP datagram relay ────────────────────────────────────────────────────

/// Relay UDP datagrams for a single QUIC connection.
///
/// Session ID → OS UDP socket mapping.  Incoming QUIC datagrams are forwarded
/// to the upstream UDP address; replies are sent back as QUIC datagrams.
async fn relay_udp_datagrams(conn: quinn::Connection) {
    let sockets: Arc<Mutex<HashMap<u32, Arc<tokio::net::UdpSocket>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    loop {
        let dg = match conn.read_datagram().await {
            Ok(d) => d,
            Err(_) => break,
        };

        let msg = match UdpMessage::decode(dg) {
            Ok(m) => m,
            Err(e) => { warn!("bad UdpMessage: {e}"); continue; }
        };

        // Only handle single-fragment datagrams for simplicity (fragments < MAX_DATAGRAM_SIZE)
        let session_id = msg.session_id;
        let addr = msg.addr.clone();
        let payload = msg.data.clone();
        let socks2 = sockets.clone();
        let conn2 = conn.clone();

        tokio::spawn(async move {
            let target = format!("{}:{}", addr.host(), addr.port());
            let socket = {
                let mut map = socks2.lock().await;
                if let Some(s) = map.get(&session_id) {
                    s.clone()
                } else {
                    let s = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                        Ok(s) => Arc::new(s),
                        Err(e) => { warn!("UDP bind: {e}"); return; }
                    };
                    map.insert(session_id, s.clone());
                    // Receiver task: upstream replies → QUIC datagrams
                    let recv_sock = s.clone();
                    let c = conn2.clone();
                    let a = addr.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 65535];
                        loop {
                            let Ok((n, from)) = recv_sock.recv_from(&mut buf).await else { break };
                            let from_addr = match from.ip() {
                                IpAddr::V4(ip) => HysteriaAddr::V4(
                                    format!("{ip}:{}", from.port()).parse().unwrap_or(from)),
                                IpAddr::V6(ip) => HysteriaAddr::V6(
                                    format!("[{ip}]:{}", from.port()).parse().unwrap_or(from)),
                            };
                            let resp = UdpMessage {
                                session_id, packet_id: 0, fragment_id: 0, fragment_count: 1,
                                addr: from_addr,
                                data: Bytes::copy_from_slice(&buf[..n]),
                            };
                            if c.send_datagram(resp.encode()).is_err() { break; }
                        }
                    });
                    s
                }
            };
            let _ = socket.send_to(&payload, &target).await;
        });
    }
}
