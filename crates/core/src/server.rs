use axum::http::{header, HeaderValue, Method};
use axum::{routing::get, Json, Router};
use serde_json::json;
use sqlx::SqlitePool;
use std::ffi::OsString;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener as TokioTcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::info;

use crate::embeddings::Embedder;
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
        // DELETE arrived with the cart routes. A browser preflights it, so
        // omitting it here means the request never reaches the handler: the
        // preflight fails, `fetch` rejects, and removing from the cart looks
        // like the server went away.
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE])
}

/// The whole HTTP surface: health, plus the project routes from [`crate::api`].
///
/// The pool is router state rather than a parameter on each handler, so a
/// handler that needs the database says so in its signature and a handler that
/// does not cannot reach for it.
///
/// The embedder is optional: a machine with no model installed still serves
/// every route, and the scorer falls back to a neutral S1.
pub fn router(pool: SqlitePool, embedder: Option<Arc<Embedder>>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .merge(crate::api::router(pool, embedder))
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

/// Bind a listener and build the server, without running it yet.
///
/// Split out of [`serve`] so a test can bind to port `0`, learn the port the
/// operating system chose, and then drive the *real* server over a socket.
/// Testing `router` through `oneshot` covers the handlers; this covers the
/// wiring from a pool to a listening socket, which is the part that had no
/// test at all.
///
/// The returned future completes when the listener closes or the serve loop
/// fails, so a caller can choose between awaiting it and dropping it.
pub async fn start(
    pool: SqlitePool,
    host: &str,
    port: u16,
) -> KfxResult<(SocketAddr, impl std::future::Future<Output = KfxResult<()>>)> {
    let listener = TokioTcpListener::bind((host, port)).await?;
    let addr = listener.local_addr()?;
    let app = router(pool, crate::embeddings::load_default());

    info!("Kinexfluence server listening on {}", addr);

    let server = async move {
        axum::serve(listener, app).await?;
        Ok(())
    };

    Ok((addr, server))
}

/// Starts the Axum server on the configured host and port.
/// If the configured port is taken, it will try the next available port.
///
/// The server owns the connection pool: it opens and migrates the database
/// named by `config.storage` and hands a clone to every handler through router
/// state. A caller that already has a pool should call [`start`] directly.
pub async fn serve(config: Config) -> KfxResult<()> {
    let host = &config.server.host;
    let port = find_available_port(host, config.server.port)?;

    let path = expand_home(&config.storage.path);
    let pool = crate::storage::open(&path, &config.storage).await?;
    // A server that started against an out-of-date schema would answer every
    // project request with a 500 about a missing column, which is a much worse
    // error than refusing to start.
    crate::storage::migrate(&pool).await?;

    let (_addr, server) = start(pool, host, port).await?;
    server.await
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
        let app = router(pool, None);
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
        let app = router(pool, None);
        let request = Request::builder()
            .uri("/v1/nonexistent")
            .body(Body::empty())?;
        let response = app.oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        Ok(())
    }

    #[sqlx::test(migrations = "../../schema/migrations")]
    async fn cors_allows_the_ui_origin(pool: SqlitePool) -> anyhow::Result<()> {
        let app = router(pool, None);
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
        let app = router(pool, None);
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

    #[sqlx::test(migrations = "../../schema/migrations")]
    async fn cors_allows_delete_for_the_ui(pool: SqlitePool) -> anyhow::Result<()> {
        let app = router(pool, None);

        // A real preflight: the browser sends OPTIONS before a DELETE and
        // refuses to send the request unless DELETE comes back in the allowed
        // methods. Omitting it leaves the handler unreachable from the UI while
        // the endpoint tests, which speak HTTP directly, keep passing.
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/v1/projects/p/cart/i")
            .header("origin", "http://localhost:5173")
            .header("access-control-request-method", "DELETE")
            .body(Body::empty())?;
        let response = app.oneshot(request).await?;

        let allowed = response
            .headers()
            .get("access-control-allow-methods")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();

        assert!(
            allowed.contains("DELETE"),
            "removing from the cart is a DELETE, and a browser preflights it; \
             allowed methods were {allowed:?}"
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
