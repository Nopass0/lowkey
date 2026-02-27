fn main() {
    // Forward LOWKEY_SERVER_IP to the Rust binary as a compile-time constant.
    // Set this env var before building to embed the default server IP:
    //   LOWKEY_SERVER_IP=1.2.3.4 npm run tauri:build
    if let Ok(ip) = std::env::var("LOWKEY_SERVER_IP") {
        if !ip.is_empty() {
            println!("cargo:rustc-env=LOWKEY_SERVER_IP={ip}");
        }
    }
    // Rebuild if the env var changes
    println!("cargo:rerun-if-env-changed=LOWKEY_SERVER_IP");

    tauri_build::build()
}
