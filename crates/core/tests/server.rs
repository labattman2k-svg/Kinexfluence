//! Integration test for the server's bind-to-socket path.
//!
//! The unit tests inside `server.rs` drive `router` through `oneshot`, which
//! covers the handlers but never opens a socket. This one binds a real
//! listener on port 0 and speaks HTTP to it, so the whole chain —
//! `SqlitePool` → `Router` → `TcpListener` → response bytes — is exercised.
//!
//! That chain is what had no test before: `serve()` opens the pool, migrates
//! it, and hands it to the router, and a mistake anywhere in it would have
//! been found by a user rather than by CI.

use kfx_core::{server, StorageConfig};
use std::net::SocketAddr;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// One HTTP/1.1 GET over a fresh connection.
///
/// Hand-rolled rather than pulling in a client crate: the point is to talk to
/// the socket, and a client library is a dev-dependency this change may not
/// add. `Connection: close` makes the read terminate at end of body.
async fn get(addr: SocketAddr, path: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(addr).await?;

    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    Ok(response)
}

#[tokio::test]
async fn start_binds_and_answers_health() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("server-test.db");
    let config = StorageConfig {
        path: db_path.to_string_lossy().to_string(),
        use_encryption: false,
    };

    let pool = kfx_core::storage::open(&db_path, &config).await?;
    kfx_core::storage::migrate(&pool).await?;

    // Port 0: the operating system picks, so this cannot collide with a
    // developer's running server or with anything else in the suite.
    let (addr, running) = server::start(pool, "127.0.0.1", 0).await?;
    assert_ne!(
        addr.port(),
        0,
        "a bound listener has to report the port it actually got"
    );

    let handle = tokio::spawn(running);

    let health = get(addr, "/v1/health").await?;
    assert!(
        health.starts_with("HTTP/1.1 200"),
        "health should answer 200, got:\n{health}"
    );
    assert!(
        health.contains("\"status\":\"ok\""),
        "health should report ok, got:\n{health}"
    );

    // A second request, on a second connection, against a route that needs the
    // database. Health alone would pass even if the pool never reached the
    // router.
    let projects = get(addr, "/v1/projects").await?;
    assert!(
        projects.starts_with("HTTP/1.1 200"),
        "projects should answer 200, got:\n{projects}"
    );
    assert!(
        projects.contains("[]"),
        "a freshly migrated database has no projects, got:\n{projects}"
    );

    handle.abort();

    Ok(())
}
