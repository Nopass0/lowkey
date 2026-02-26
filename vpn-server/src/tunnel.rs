//! UDP tunnel — bidirectional encrypted packet forwarding between TUN and peers.
//!
//! Two long-running async tasks handle the data path:
//!
//! ```text
//!  ┌──────────────┐   task_tun_to_udp   ┌──────────────┐
//!  │   TUN device │ ──────────────────> │  UDP socket  │
//!  │  (IP packets)│                     │  (internet)  │
//!  │              │ <────────────────── │              │
//!  └──────────────┘   task_udp_to_tun   └──────────────┘
//! ```
//!
//! ## TUN → UDP (outbound to client)
//! 1. Read a plaintext IP packet from the TUN device.
//! 2. Look up the destination peer by the packet's IPv4 destination address.
//! 3. Apply the peer's token-bucket rate limiter — drop if throttled.
//! 4. Encrypt with ChaCha20-Poly1305 and send to the peer's last UDP address.
//!
//! ## UDP → TUN (inbound from client)
//! 1. Receive a UDP datagram from any peer.
//! 2. Extract the 4-byte client VPN IP prefix that identifies the sender.
//! 3. Update the peer's UDP endpoint (NAT traversal / IP roaming).
//! 4. Decrypt the payload and write the plaintext IP packet to the TUN device.

use std::{net::Ipv4Addr, sync::Arc, sync::atomic::Ordering};

use anyhow::Result;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::UdpSocket, sync::Mutex};
use tracing::{error, warn};
use vpn_common::parse_dest_ipv4;

use crate::state::Shared;

// ── TUN → UDP ─────────────────────────────────────────────────────────────────

/// Forward outgoing (server → client) plaintext IP packets from TUN to UDP.
///
/// This task runs for the lifetime of the server.  It reads raw IP packets
/// from the TUN device, encrypts each one with the destination peer's session
/// key, and sends the ciphertext to the peer's last known UDP address.
///
/// # Rate limiting
/// Before encrypting, the packet is passed through the peer's token-bucket
/// rate limiter.  If the bucket is empty the packet is silently dropped (this
/// is intentional — TCP congestion control will slow the sender down).
///
/// # Arguments
/// * `tun` — async read half of the TUN device
/// * `udp` — shared UDP socket (bound to `0.0.0.0:udp_port`)
/// * `state` — shared server state (peer table, stats)
pub async fn task_tun_to_udp(
    mut tun: impl AsyncReadExt + Unpin,
    udp: Arc<UdpSocket>,
    state: Shared,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        // Read one IP packet from the TUN device (blocks until data is available)
        let n = tun.read(&mut buf).await?;
        if n == 0 {
            break; // TUN device closed
        }
        let pkt = &buf[..n];

        // Parse the IPv4 destination address from the packet header
        let dest = match parse_dest_ipv4(pkt) {
            Some(ip) => ip,
            None => continue, // not an IPv4 packet — skip
        };

        // Only forward packets destined for our VPN subnet (10.0.0.0/24)
        let o = dest.octets();
        if o[0] != 10 || o[1] != 0 || o[2] != 0 {
            continue; // not a VPN client — let the kernel handle it
        }

        // Look up the peer by destination VPN IP
        let peer = match state.peers.get(&dest) {
            Some(e) => e.clone(),
            None => continue, // no registered peer at this IP
        };

        // Peer must have announced its UDP endpoint at least once
        let ep = match *peer.endpoint.read().await {
            Some(ep) => ep,
            None => continue, // client hasn't sent a packet yet
        };

        // Token-bucket rate limiting (bytes_out direction)
        let limit = peer.limit_bps.load(Ordering::Relaxed);
        {
            let mut bucket = peer.bucket.lock().await;
            if !bucket.consume(n, limit) {
                continue; // throttled — drop packet silently
            }
        }

        // Encrypt and send
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

/// Forward incoming (client → server) encrypted UDP datagrams to the TUN device.
///
/// This task runs for the lifetime of the server.  It receives encrypted UDP
/// datagrams, identifies the sender by the 4-byte VPN IP prefix, decrypts
/// the payload, and writes the plaintext IP packet to the TUN device so the
/// Linux kernel can route it.
///
/// # Wire format (client → server)
/// ```text
/// [4 B : client VPN IP]  [12 B : nonce]  [N B : ciphertext+tag]
/// ```
/// The 4-byte VPN IP prefix is added by the client (not part of the encrypted
/// payload) so the server can look up the correct decryption key without a
/// full-table scan.
///
/// # Endpoint learning
/// The server tracks the client's current UDP source address.  If it changes
/// (e.g. after a NAT rebind or IP roam) the endpoint is updated transparently,
/// enabling seamless handover without reconnection.
///
/// # Arguments
/// * `udp` — shared UDP socket
/// * `tun` — async write half of the TUN device, wrapped in a mutex
/// * `state` — shared server state (peer table, stats)
pub async fn task_udp_to_tun(
    udp: Arc<UdpSocket>,
    tun: Arc<Mutex<impl AsyncWriteExt + Unpin>>,
    state: Shared,
) -> Result<()> {
    // Extra headroom for the 16-byte AEAD tag
    let mut buf = vec![0u8; 65536 + 64];
    loop {
        let (n, src) = udp.recv_from(&mut buf).await?;

        // Minimum: 4B VPN IP + 12B nonce + 1B plaintext + 16B tag = 33B
        if n < 5 {
            continue; // too short to be a valid packet
        }

        // First 4 bytes are the client's assigned VPN IP (routing header)
        let vpn_ip = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
        let payload = &buf[4..n]; // nonce + ciphertext

        let peer = match state.peers.get(&vpn_ip) {
            Some(e) => e.clone(),
            None => {
                warn!("Unknown VPN IP {vpn_ip} from {src}");
                continue; // no registered peer — discard
            }
        };

        // Update the peer's UDP endpoint if it has changed (NAT roaming support)
        {
            let mut ep = peer.endpoint.write().await;
            if *ep != Some(src) {
                // Remove the old reverse mapping before adding the new one
                if let Some(old) = *ep {
                    state.endpoints.remove(&old);
                }
                *ep = Some(src);
                state.endpoints.insert(src, vpn_ip);
                state.push_log(format!("Endpoint updated {vpn_ip} → {src}"));
            }
        }

        // Decrypt the payload (ChaCha20-Poly1305 authenticates and decrypts)
        let plain = match peer.crypto.decrypt(payload) {
            Some(p) => p,
            None => {
                warn!("Decryption failed from {src}");
                continue; // authentication failed — discard
            }
        };

        // Ignore keepalive probes sent by the client to establish the endpoint
        if plain == b"hello" {
            continue;
        }

        // Count traffic and write to TUN
        peer.bytes_in.fetch_add(plain.len() as u64, Ordering::Relaxed);
        state.total_bytes_in.fetch_add(plain.len() as u64, Ordering::Relaxed);

        let mut tw = tun.lock().await;
        if let Err(e) = tw.write_all(&plain).await {
            error!("TUN write error: {e}");
        }
    }
}
