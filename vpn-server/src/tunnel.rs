//! UDP tunnel — bidirectional encrypted packet forwarding between TUN and peers.

use std::{net::Ipv4Addr, sync::Arc, sync::atomic::Ordering};

use anyhow::Result;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::UdpSocket, sync::Mutex};
use tracing::{error, warn};
use vpn_common::parse_dest_ipv4;

use crate::state::Shared;

// ── TUN → UDP ─────────────────────────────────────────────────────────────────

/// Read decrypted IP packets from TUN; encrypt and forward to the right peer.
pub async fn task_tun_to_udp(
    mut tun: impl AsyncReadExt + Unpin,
    udp: Arc<UdpSocket>,
    state: Shared,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let n = tun.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let pkt = &buf[..n];

        let dest = match parse_dest_ipv4(pkt) {
            Some(ip) => ip,
            None => continue,
        };

        // Only relay packets destined for VPN subnet (10.0.0.0/24)
        let o = dest.octets();
        if o[0] != 10 || o[1] != 0 || o[2] != 0 {
            continue;
        }

        let peer = match state.peers.get(&dest) {
            Some(e) => e.clone(),
            None => continue,
        };

        let ep = match *peer.endpoint.read().await {
            Some(ep) => ep,
            None => continue, // Peer hasn't announced its UDP endpoint yet
        };

        // Rate limiting (send direction = bytes_out)
        let limit = peer.limit_bps.load(Ordering::Relaxed);
        {
            let mut bucket = peer.bucket.lock().await;
            if !bucket.consume(n, limit) {
                continue; // throttled — drop packet
            }
        }

        let encrypted = peer.crypto.encrypt(pkt);
        if let Err(e) = udp.send_to(&encrypted, ep).await {
            warn!("UDP send to {ep} failed: {e}");
        } else {
            peer.bytes_out.fetch_add(n as u64, Ordering::Relaxed);
            state.total_bytes_out.fetch_add(n as u64, Ordering::Relaxed);
        }
    }
    Ok(())
}

// ── UDP → TUN ─────────────────────────────────────────────────────────────────

/// Receive encrypted UDP packets from peers; decrypt and inject into TUN.
///
/// Wire format (client → server):
///   [4 B: client VPN IP] [12 B: nonce] [ciphertext + 16 B tag]
pub async fn task_udp_to_tun(
    udp: Arc<UdpSocket>,
    tun: Arc<Mutex<impl AsyncWriteExt + Unpin>>,
    state: Shared,
) -> Result<()> {
    let mut buf = vec![0u8; 65536 + 64];
    loop {
        let (n, src) = udp.recv_from(&mut buf).await?;
        if n < 5 {
            continue;
        }

        // First 4 bytes = client's assigned VPN IP (routing header)
        let vpn_ip = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
        let payload = &buf[4..n];

        let peer = match state.peers.get(&vpn_ip) {
            Some(e) => e.clone(),
            None => {
                warn!("Unknown VPN IP {vpn_ip} from {src}");
                continue;
            }
        };

        // Learn / refresh peer endpoint
        {
            let mut ep = peer.endpoint.write().await;
            if *ep != Some(src) {
                if let Some(old) = *ep {
                    state.endpoints.remove(&old);
                }
                *ep = Some(src);
                state.endpoints.insert(src, vpn_ip);
                state.push_log(format!("Endpoint updated {vpn_ip} → {src}"));
            }
        }

        let plain = match peer.crypto.decrypt(payload) {
            Some(p) => p,
            None => {
                warn!("Decryption failed from {src}");
                continue;
            }
        };

        // Skip keepalive probes
        if plain == b"hello" {
            continue;
        }

        peer.bytes_in.fetch_add(plain.len() as u64, Ordering::Relaxed);
        state.total_bytes_in.fetch_add(plain.len() as u64, Ordering::Relaxed);

        let mut tw = tun.lock().await;
        if let Err(e) = tw.write_all(&plain).await {
            error!("TUN write error: {e}");
        }
    }
}
