#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

#[tokio::main]
async fn main() {
    let config = kfx_core::Config::default();

    // Spawn the core server in the background
    let server_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = kfx_core::server::serve(server_config).await {
            eprintln!("Server error: {}", e);
        }
    });

    tauri::Builder::default()
        .setup(|app| {
            // Store config in app state for later use
            app.manage(config);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Kinexfluence");
}
