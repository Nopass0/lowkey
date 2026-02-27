//! VPN connection management.
//!
//! On Linux: manages the vpn-client binary (SOCKS5/TUN mode).
//! On Windows: uses the Windows WinTUN-based approach via the client binary.

use anyhow::Result;
use std::sync::Mutex;
use std::process::Child;

static VPN_PROCESS: Mutex<Option<Child>> = Mutex::new(None);

/// Start the VPN connection by launching the client binary.
pub async fn connect(server_ip: &str, port: u16, psk: &str, vpn_ip: &str) -> Result<()> {
    disconnect().await.ok(); // Ensure no existing connection

    // Get the path to the bundled client binary
    let client_bin = get_client_binary_path();

    let child = std::process::Command::new(&client_bin)
        .args([
            "--server", &format!("{server_ip}:{port}"),
            "--psk", psk,
            "--vpn-ip", vpn_ip,
            "--mode", "tun",
            "--no-tui",
        ])
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start VPN client: {e}"))?;

    *VPN_PROCESS.lock().unwrap() = Some(child);
    Ok(())
}

/// Stop the VPN connection.
pub async fn disconnect() -> Result<()> {
    let mut guard = VPN_PROCESS.lock().unwrap();
    if let Some(mut child) = guard.take() {
        child.kill().ok();
        child.wait().ok();

        // Clean up routes/TUN interface
        cleanup_vpn_routes();
    }
    Ok(())
}

/// Get the path to the bundled VPN client binary.
fn get_client_binary_path() -> String {
    #[cfg(target_os = "windows")]
    { "lowkey-vpn-client.exe".to_string() }

    #[cfg(target_os = "linux")]
    { "./lowkey-vpn-client".to_string() }

    #[cfg(target_os = "macos")]
    { "./lowkey-vpn-client".to_string() }
}

/// Clean up VPN routes/interface after disconnect.
fn cleanup_vpn_routes() {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("ip")
            .args(["link", "del", "tun0"])
            .output();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("netsh")
            .args(["interface", "set", "interface", "name=\"Lowkey VPN\"", "admin=disable"])
            .output();
    }
}
