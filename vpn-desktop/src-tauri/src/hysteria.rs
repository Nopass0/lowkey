//! Embedded Hysteria2 QUIC client.
//!
//! Connects to the Hysteria2 server, authenticates via HTTP/3, and exposes
//! a local SOCKS5 proxy that tunnels all connections through the server.
//!
//! # Usage
//! ```rust
//! let conn = hysteria::connect("vpn.example.com", 8388, "password", 10808).await?;
//! // SOCKS5 proxy is now live at 127.0.0.1:10808
//! // ...
//! conn.abort(); // disconnect
//! ```

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use http::Request;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use vpn_common::hysteria::{auth_headers, HysteriaAddr, TcpRequest, TcpResponse, AUTH_SUCCESS_STATUS};

// ─── Public handle ────────────────────────────────────────────────────────────

/// Handle to an active Hysteria2 SOCKS5 proxy.
/// Call `.abort()` to stop the proxy and release the port.
pub struct HysteriaConnection {
    handle: JoinHandle<()>,
    /// Local port the SOCKS5 proxy is listening on.
    pub socks5_port: u16,
    /// Remote Hysteria2 server host.
    pub server_host: String,
}

impl HysteriaConnection {
    /// Stop the SOCKS5 proxy and close the QUIC connection.
    pub fn abort(self) {
        self.handle.abort();
    }
}

// ─── Entry point ──────────────────────────────────────────────────────────────

/// Connect to a Hysteria2 server and start a local SOCKS5 proxy.
///
/// This function:
/// 1. Establishes a QUIC connection to `server_host:hysteria_port`
/// 2. Authenticates via HTTP/3 with `password`
/// 3. Binds a SOCKS5 listener on `127.0.0.1:socks5_port`
/// 4. Spawns a background task that forwards connections
///
/// Returns a [`HysteriaConnection`] handle. Call `.abort()` to stop.
pub async fn connect(
    server_host: &str,
    hysteria_port: u16,
    password: &str,
    socks5_port: u16,
) -> Result<HysteriaConnection> {
    let endpoint = build_quic_client()?;

    let addr: SocketAddr = tokio::net::lookup_host(format!("{server_host}:{hysteria_port}"))
        .await
        .context("DNS lookup failed")?
        .next()
        .context("DNS returned no addresses")?;

    let conn = endpoint
        .connect(addr, server_host)
        .context("QUIC connect init")?
        .await
        .context("QUIC handshake failed")?;

    authenticate_h3(&conn, password)
        .await
        .context("Hysteria2 auth failed")?;

    let conn = Arc::new(conn);

    let listener = TcpListener::bind(format!("127.0.0.1:{socks5_port}"))
        .await
        .with_context(|| format!("SOCKS5 bind failed on port {socks5_port}"))?;

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _peer) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };
            let c = conn.clone();
            tokio::spawn(async move {
                let _ = handle_socks5(stream, c).await;
            });
        }
    });

    Ok(HysteriaConnection {
        handle,
        socks5_port,
        server_host: server_host.to_owned(),
    })
}

// ─── TLS: skip-verify ────────────────────────────────────────────────────────

/// Accepts any server certificate — traffic is still TLS-encrypted.
#[derive(Debug)]
struct SkipVerify;

impl rustls::client::danger::ServerCertVerifier for SkipVerify {
    fn verify_server_cert(
        &self,
        _ee: &rustls::pki_types::CertificateDer,
        _inters: &[rustls::pki_types::CertificateDer],
        _sn: &rustls::pki_types::ServerName,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _msg: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _msg: &[u8],
        _cert: &rustls::pki_types::CertificateDer,
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

fn build_quic_client() -> Result<quinn::Endpoint> {
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerify))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];

    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(Duration::from_secs(30).try_into()?));
    transport.datagram_receive_buffer_size(Some(4 * 1024 * 1024));
    transport.datagram_send_buffer_size(4 * 1024 * 1024);
    transport.keep_alive_interval(Some(Duration::from_secs(10)));

    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .context("quinn: client crypto build")?;
    let mut cfg = quinn::ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(Arc::new(transport));

    let mut ep = quinn::Endpoint::client("0.0.0.0:0".parse()?)
        .context("quinn: bind")?;
    ep.set_default_client_config(cfg);
    Ok(ep)
}

// ─── HTTP/3 authentication ────────────────────────────────────────────────────

async fn authenticate_h3(conn: &quinn::Connection, password: &str) -> Result<()> {
    let h3_conn = h3_quinn::Connection::new(conn.clone());
    let (mut driver, mut send_request) = h3::client::new(h3_conn)
        .await
        .context("H3 client init")?;

    tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let req = Request::builder()
        .method("POST")
        .uri("/auth")
        .header(auth_headers::AUTH, password)
        .header(auth_headers::CC_RX, "0")
        .header(auth_headers::PADDING, "00")
        .body(())
        .context("build auth request")?;

    let mut stream = send_request.send_request(req).await.context("H3 send_request")?;
    stream.finish().await.context("H3 stream finish")?;

    let resp = stream.recv_response().await.context("H3 recv_response")?;
    let status = resp.status().as_u16();
    if status != AUTH_SUCCESS_STATUS {
        bail!("Hysteria2 auth rejected (HTTP {status}) — wrong password?");
    }
    Ok(())
}

// ─── SOCKS5 handler ──────────────────────────────────────────────────────────

async fn handle_socks5(mut client: TcpStream, quic: Arc<quinn::Connection>) -> Result<()> {
    // SOCKS5 greeting
    let mut hdr = [0u8; 2];
    client.read_exact(&mut hdr).await?;
    if hdr[0] != 5 {
        bail!("not SOCKS5");
    }
    let n = hdr[1] as usize;
    let mut methods = vec![0u8; n];
    client.read_exact(&mut methods).await?;
    client.write_all(&[5u8, 0u8]).await?; // no-auth

    // CONNECT request
    let mut req = [0u8; 4];
    client.read_exact(&mut req).await?;
    if req[1] != 1 {
        client.write_all(&[5u8, 7u8, 0u8, 1u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8]).await.ok();
        bail!("unsupported SOCKS5 cmd {}", req[1]);
    }

    let addr = match req[3] {
        0x01 => {
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip).await?;
            let mut p = [0u8; 2];
            client.read_exact(&mut p).await?;
            HysteriaAddr::V4(
                format!("{}.{}.{}.{}:{}", ip[0], ip[1], ip[2], ip[3], u16::from_be_bytes(p))
                    .parse()?,
            )
        }
        0x03 => {
            let mut lb = [0u8; 1];
            client.read_exact(&mut lb).await?;
            let mut host = vec![0u8; lb[0] as usize];
            client.read_exact(&mut host).await?;
            let mut p = [0u8; 2];
            client.read_exact(&mut p).await?;
            HysteriaAddr::Name(String::from_utf8(host)?, u16::from_be_bytes(p))
        }
        0x04 => {
            let mut ip = [0u8; 16];
            client.read_exact(&mut ip).await?;
            let mut p = [0u8; 2];
            client.read_exact(&mut p).await?;
            HysteriaAddr::V6(
                format!("[{}]:{}", std::net::Ipv6Addr::from(ip), u16::from_be_bytes(p))
                    .parse()?,
            )
        }
        t => bail!("unknown addr type {t:#x}"),
    };

    // Open QUIC bidirectional stream
    let (mut qs, mut qr) = quic.open_bi().await.context("QUIC open_bi")?;
    qs.write_all(&TcpRequest { addr }.encode())
        .await
        .context("TcpRequest write")?;

    // Read server response
    let mut rbuf = vec![0u8; 256];
    let n = qr.read(&mut rbuf).await?.unwrap_or(0);
    let tr = TcpResponse::decode(Bytes::copy_from_slice(&rbuf[..n]))?;
    if !tr.ok {
        client
            .write_all(&[5u8, 4u8, 0u8, 1u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8])
            .await
            .ok();
        bail!("server refused: {}", tr.message);
    }

    // Tell SOCKS5 client: connection established
    client
        .write_all(&[5u8, 0u8, 0u8, 1u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8])
        .await?;

    // Bidirectional relay
    let (mut cr, mut cw) = client.into_split();
    let c2q = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match cr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if qs.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        qs.finish().ok();
    });
    let q2c = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match qr.read(&mut buf).await {
                Ok(Some(n @ 1..)) => {
                    if cw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });
    let _ = tokio::join!(c2q, q2c);
    Ok(())
}
