use axum::{routing::get, Json, Router};
use serde_json::json;
use std::net::TcpListener;
use tokio::net::TcpListener as TokioTcpListener;
use tracing::info;

use crate::Config;

pub fn router() -> Router {
    Router::new().route("/v1/health", get(health))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "schema_version": 1 }))
}

/// Finds an available port starting from the given port.
/// Returns the first available port greater than or equal to start_port.
fn find_available_port(host: &str, start_port: u16) -> KfxResult<u16> {
    for port in start_port..65535 {
        if let Ok(listener) = TcpListener::bind((host, port)) {
            drop(listener);
            return Ok(port);
        }
    }
    Err(crate::KfxError::Internal("No available port found".to_string()))
}

/// Starts the Axum server on the configured host and port.
/// If the configured port is taken, it will try the next available port.
pub async fn serve(config: Config) -> KfxResult<()> {
    let host = &config.server.host;
    let port = find_available_port(host, config.server.port)?;

    let addr = format!("{}:{}", host, port);
    let listener = TokioTcpListener::bind(&addr).await?;
    let app = router();

    info!("Kinexfluence server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
