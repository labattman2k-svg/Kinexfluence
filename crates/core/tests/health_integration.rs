use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use kfx_core::server;
use tower::ServiceExt;

#[tokio::test]
async fn test_health_endpoint() {
    let app = server::router();

    let request = Request::builder()
        .uri("/v1/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["schema_version"], 1);
}
