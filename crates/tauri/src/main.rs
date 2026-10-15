#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

use kfx_core::{server, Config};

#[tokio::main]
async fn main() -> tauri::Result<()> {
    let config = Config::default();

    // Spawn the core server in the background
    let server_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = server::serve(server_config).await {
            eprintln!("Server error: {}", e);
        }
    });

    tauri::Builder::default()
        .setup(|app| {
            app.manage(config);
            Ok(())
        })
        .run(tauri::generate_context!())
}
