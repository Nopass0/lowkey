//! Lowkey VPN Desktop — Tauri backend.
//!
//! Provides Tauri commands for:
//! - VPN connection/disconnection (via OS VPN or client binary)
//! - Session persistence (via tauri-plugin-store)
//! - SBP payment status polling
//! - System tray management

use std::sync::{Arc, Mutex};
use tauri::{
    AppHandle, Manager, State,
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
};

mod vpn;
mod api;

/// Global VPN connection state.
pub struct VpnState {
    pub connected: Mutex<bool>,
    pub vpn_ip: Mutex<Option<String>>,
    pub server_ip: Mutex<Option<String>>,
}

impl Default for VpnState {
    fn default() -> Self {
        VpnState {
            connected: Mutex::new(false),
            vpn_ip: Mutex::new(None),
            server_ip: Mutex::new(None),
        }
    }
}

/// Toggle VPN connection.
#[tauri::command]
async fn toggle_vpn(
    state: State<'_, Arc<VpnState>>,
    app: AppHandle,
    token: String,
    api_url: String,
) -> Result<serde_json::Value, String> {
    let connected = *state.connected.lock().unwrap();

    if connected {
        // Disconnect
        vpn::disconnect().await.map_err(|e| e.to_string())?;
        *state.connected.lock().unwrap() = false;
        *state.vpn_ip.lock().unwrap() = None;

        update_tray_icon(&app, false);

        Ok(serde_json::json!({ "connected": false }))
    } else {
        // Connect: register peer and start VPN
        let peer_info = api::register_peer(&api_url, &token)
            .await
            .map_err(|e| e.to_string())?;

        let vpn_ip = peer_info["vpn_ip"].as_str().unwrap_or("").to_string();
        let server = peer_info["server_ip"].as_str().unwrap_or("").to_string();
        let port = peer_info["port"].as_u64().unwrap_or(51820) as u16;
        let psk = peer_info["psk"].as_str().unwrap_or("").to_string();

        vpn::connect(&server, port, &psk, &vpn_ip)
            .await
            .map_err(|e| e.to_string())?;

        *state.connected.lock().unwrap() = true;
        *state.vpn_ip.lock().unwrap() = Some(vpn_ip.clone());
        *state.server_ip.lock().unwrap() = Some(server.clone());

        update_tray_icon(&app, true);

        Ok(serde_json::json!({
            "connected": true,
            "vpn_ip": vpn_ip,
            "server_ip": server,
        }))
    }
}

/// Get current VPN status.
#[tauri::command]
fn vpn_status(state: State<'_, Arc<VpnState>>) -> serde_json::Value {
    let connected = *state.connected.lock().unwrap();
    let vpn_ip = state.vpn_ip.lock().unwrap().clone();
    let server_ip = state.server_ip.lock().unwrap().clone();
    serde_json::json!({
        "connected": connected,
        "vpn_ip": vpn_ip,
        "server_ip": server_ip,
    })
}

/// Fetch user info from the API.
#[tauri::command]
async fn get_user_info(api_url: String, token: String) -> Result<serde_json::Value, String> {
    api::get_user(&api_url, &token)
        .await
        .map_err(|e| e.to_string())
}

/// Login via the API.
#[tauri::command]
async fn api_login(
    api_url: String,
    login: String,
    password: String,
) -> Result<serde_json::Value, String> {
    api::login(&api_url, &login, &password)
        .await
        .map_err(|e| e.to_string())
}

/// Get subscription plans.
#[tauri::command]
async fn get_plans(api_url: String) -> Result<serde_json::Value, String> {
    api::get_plans(&api_url)
        .await
        .map_err(|e| e.to_string())
}

/// Create SBP payment.
#[tauri::command]
async fn create_sbp_payment(
    api_url: String,
    token: String,
    amount: f64,
    purpose: String,
    plan_id: Option<String>,
) -> Result<serde_json::Value, String> {
    api::create_payment(&api_url, &token, amount, &purpose, plan_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Poll SBP payment status.
#[tauri::command]
async fn poll_payment_status(
    api_url: String,
    token: String,
    payment_id: u64,
) -> Result<serde_json::Value, String> {
    api::payment_status(&api_url, &token, payment_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get referral stats.
#[tauri::command]
async fn get_referral_stats(api_url: String, token: String) -> Result<serde_json::Value, String> {
    api::referral_stats(&api_url, &token)
        .await
        .map_err(|e| e.to_string())
}

/// Check for an available app update.
/// Returns null if no update is available or when running a debug build.
#[tauri::command]
async fn check_for_update(
    api_url: String,
    current_version: String,
) -> Result<Option<serde_json::Value>, String> {
    if cfg!(debug_assertions) {
        return Ok(None);
    }
    let release = match api::get_latest_release(&api_url, "windows").await {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let latest_version = release["version"].as_str().unwrap_or("0.0.0");
    if compare_versions(latest_version, &current_version) > 0 {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

fn compare_versions(v1: &str, v2: &str) -> i32 {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let p1 = parse(v1);
    let p2 = parse(v2);
    let len = p1.len().max(p2.len());
    for i in 0..len {
        let a = *p1.get(i).unwrap_or(&0);
        let b = *p2.get(i).unwrap_or(&0);
        if a != b {
            return if a > b { 1 } else { -1 };
        }
    }
    0
}

/// Update tray icon based on VPN state.
fn update_tray_icon(app: &AppHandle, connected: bool) {
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = if connected {
            "Lowkey VPN — Подключён"
        } else {
            "Lowkey VPN — Отключён"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

/// App entry point (called by Tauri).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let vpn_state = Arc::new(VpnState::default());
    let start_hidden = std::env::args().any(|arg| arg == "--hidden");

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(vpn_state)
        .invoke_handler(tauri::generate_handler![
            toggle_vpn,
            vpn_status,
            get_user_info,
            api_login,
            get_plans,
            create_sbp_payment,
            poll_payment_status,
            get_referral_stats,
            check_for_update,
        ])
        .setup(move |app| {
            // Create system tray
            let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Открыть", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::with_id("main")
                .menu(&menu)
                .tooltip("Lowkey VPN")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        // Disconnect VPN before quitting
                        let state: State<Arc<VpnState>> = app.state();
                        if *state.connected.lock().unwrap() {
                            tokio::runtime::Handle::current().block_on(vpn::disconnect()).ok();
                        }
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                if start_hidden {
                    let _ = window.hide();
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }

                // Handle window close → minimize to tray
                let window_ = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_.hide();
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Error running Tauri app");
}
