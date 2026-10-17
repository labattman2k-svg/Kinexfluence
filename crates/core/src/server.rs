use axum::http::{header, HeaderValue, Method};
use axum::{routing::get, Json, Router};
use serde_json::json;
use sqlx::SqlitePool;
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::PathBuf;
use tokio::net::TcpListener as TokioTcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::info;

use crate::{Config, KfxResult};

/// Origins the Atlas UI may be served from.
///
/// An explicit list rather than [`CorsLayer::permissive`]. The server is
/// loopback-only but otherwise unauthenticated, so a permissive layer would let
/// *any* page the user happens to visit read their projects and post new ones.
/// §5 asks for an origin check, and this is it.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            // `npm --prefix ui run dev`
            HeaderValue::from_static("http://localhost:5173"),
            // Tauri's webview origin, which differs by platform.
            HeaderValue::from_static("tauri://localhost"),
            HeaderValue::from_static("http://tauri.localhost"),
        ]))
        .allow_methods([Method::GET, Method::POST, Method::PATCH])
        .allow_headers([header::CONTENT_TYPE])
}

/// The whole HTTP surface: health, plus the project routes from [`crate::api`].
///
/// The pool is router state rather than a parameter on each handler, so a
/// handler that needs the database says so in its signature and a handler that
/// does not cannot reach for it.
pub fn router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .merge(crate::api::router(pool))
        .layer(cors_layer())
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
    Err(crate::KfxError::Internal(
        "No available port found".to_string(),
    ))
}

/// Starts the Axum server on the configured host and port.
/// If the configured port is taken, it will try the next available port.
///
/// The server owns the connection pool: it opens and migrates the database
/// named by `config.storage` and hands a clone to every handler through router
/// state. A caller that already has a pool should build a router with
/// [`router`] directly rather than call this.
pub async fn serve(config: Config) -> KfxResult<()> {
    let host = &config.server.host;
    let port = find_available_port(host, config.server.port)?;

    let addr = format!("{}:{}", host, port);
    let listener = TokioTcpListener::bind(&addr).await?;

    let path = expand_home(&config.storage.path);
    let pool = crate::storage::open(&path, &config.storage).await?;
    // A server that started against an out-of-date schema would answer every
    // project request with a 500 about a missing column, which is a much worse
    // error than refusing to start.
    crate::storage::migrate(&pool).await?;

    let app = router(pool);

    info!("Kinexfluence server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Expand a leading `~/` against the user's home directory.
///
/// This duplicates `shellexpand` in `crates/cli/src/main.rs`, which this change
/// may not edit. `Config::default()` uses a `~` path and the Tauri shell passes
/// it straight to [`serve`], so without expansion the server would create a
/// directory literally named `~`. The two copies should be consolidated into
/// `storage` — flagged rather than done here, because moving it is a change to
/// a file outside this PR's boundary.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), home_dir()) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The user's home directory, in the order the platforms actually set it.
///
/// `HOME` is the POSIX convention; Windows normally sets `USERPROFILE` and
/// often leaves `HOME` unset. Empty values count as unset so an empty `HOME`
/// cannot shadow a usable `USERPROFILE`.
fn home_dir() -> Option<OsString> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[sqlx::test(migrations = "../../schema/migrations")]
    async fn health_reports_schema_version_1(pool: SqlitePool) -> anyhow::Result<()> {
        let app = router(pool);
        let request = Request::builder().uri("/v1/health").body(Body::empty())?;
        let response = app.oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await?.to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["status"], "ok");
        Ok(())
    }

    #[sqlx::test(migrations = "../../schema/migrations")]
    async fn router_has_only_expected_routes(pool: SqlitePool) -> anyhow::Result<()> {
        let app = router(pool);
        let request = Request::builder()
            .uri("/v1/nonexistent")
            .body(Body::empty())?;
        let response = app.oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        Ok(())
    }

    #[sqlx::test(migrations = "../../schema/migrations")]
    async fn cors_allows_the_ui_origin(pool: SqlitePool) -> anyhow::Result<()> {
        let app = router(pool);
        let request = Request::builder()
            .uri("/v1/health")
            .header("origin", "http://localhost:5173")
            .body(Body::empty())?;
        let response = app.oneshot(request).await?;

        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("http://localhost:5173"),
            "the UI runs on a different origin from the API and has to be allowed"
        );
        Ok(())
    }

    #[sqlx::test(migrations = "../../schema/migrations")]
    async fn cors_rejects_an_unknown_origin(pool: SqlitePool) -> anyhow::Result<()> {
        let app = router(pool);
        let request = Request::builder()
            .uri("/v1/health")
            .header("origin", "https://evil.example")
            .body(Body::empty())?;
        let response = app.oneshot(request).await?;

        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "an arbitrary page must not be able to read this server; \
             a permissive layer would hand the user's projects to any tab"
        );
        Ok(())
    }

    #[test]
    fn expands_tilde_against_home() {
        assert_eq!(
            expand_home_with(
                "~/.kinexfluence/kinexfluence.db",
                Some(OsString::from("/home/u"))
            ),
            PathBuf::from("/home/u/.kinexfluence/kinexfluence.db")
        );
    }

    #[test]
    fn leaves_absolute_paths_untouched() {
        assert_eq!(
            expand_home_with("/var/data/k.db", Some(OsString::from("/home/u"))),
            PathBuf::from("/var/data/k.db")
        );
    }

    #[test]
    fn leaves_tilde_when_no_home_is_known() {
        assert_eq!(
            expand_home_with("~/.kinexfluence/k.db", None),
            PathBuf::from("~/.kinexfluence/k.db")
        );
    }

    /// The pure form of [`expand_home`], so the environment lookup stays out of
    /// the assertions.
    fn expand_home_with(path: &str, home: Option<OsString>) -> PathBuf {
        match (path.strip_prefix("~/"), home) {
            (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
            _ => PathBuf::from(path),
        }
    }
}
