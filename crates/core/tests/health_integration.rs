use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use kfx_core::server;
use sqlx::SqlitePool;
use tower::ServiceExt;

// The router now carries the connection pool as state, so this test needs a
// real database the way every project request does. `#[sqlx::test]` gives it a
// freshly migrated one.
#[sqlx::test(migrations = "../../schema/migrations")]
async fn test_health_endpoint(pool: SqlitePool) {
    let app = server::router(pool);

    let request = Request::builder()
        .uri("/v1/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["schema_version"], 1);
}
