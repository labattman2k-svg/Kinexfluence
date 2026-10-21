//! Project HTTP API tests.
//!
//! Every test drives the real router with `tower::ServiceExt::oneshot`, so no
//! port is bound and nothing races another test for one. The database is a
//! freshly migrated `#[sqlx::test]` pool, because most of what the API
//! promises is about how `project.rs` and SQLite behave, not about the handler
//! functions in isolation.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use kfx_core::server;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tower::ServiceExt;

/// One request through the router, returning the status and decoded body.
///
/// The router is rebuilt per call, which is cheap and keeps each call
/// independent: no test can be affected by state a previous call left in a
/// shared router.
async fn call(
    pool: &SqlitePool,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let app = server::router(pool.clone(), None);

    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .expect("request should build"),
        None => builder.body(Body::empty()).expect("request should build"),
    };

    let response = app.oneshot(request).await.expect("router should respond");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should read")
        .to_bytes();

    let decoded = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("body should be json")
    };

    (status, decoded)
}

/// Create a project through the API and return its JSON.
async fn create(pool: &SqlitePool, name: &str) -> Value {
    let (status, body) = call(
        pool,
        Method::POST,
        "/v1/projects",
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create should succeed: {body}");
    body
}

// ---------------------------------------------------------------------------
// health and listing
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn health_returns_ok(pool: SqlitePool) {
    let (status, body) = call(&pool, Method::GET, "/v1/health", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["schema_version"], 1);
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_projects_empty(pool: SqlitePool) {
    let (status, body) = call(&pool, Method::GET, "/v1/projects", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!([]),
        "an empty project table is an empty array, not a 404"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_projects_returns_created(pool: SqlitePool) {
    let created = create(&pool, "Alpha Project").await;

    let (status, body) = call(&pool, Method::GET, "/v1/projects", None).await;

    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().expect("a json array");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], created["id"]);
    assert_eq!(list[0]["name"], "Alpha Project");
    assert_eq!(list[0]["slug"], "alpha-project");
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_project_returns_201_and_id(pool: SqlitePool) {
    let (status, body) = call(
        &pool,
        Method::POST,
        "/v1/projects",
        Some(json!({
            "name": "Alpha Project",
            "brief": "Why this exists",
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        body["id"].as_str().map(str::len),
        Some(36),
        "the id should be a uuid, got {:?}",
        body["id"]
    );
    assert_eq!(body["slug"], "alpha-project");
    assert_eq!(
        body["status"], "recon",
        "a new project starts in recon, per the schema default"
    );
    assert_eq!(body["brief"], "Why this exists");
    assert!(
        body["created_at"].is_string() && body["updated_at"].is_string(),
        "timestamps should be rendered as strings, got {body}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn create_project_rejects_empty_name(pool: SqlitePool) {
    let (status, body) = call(
        &pool,
        Method::POST,
        "/v1/projects",
        Some(json!({ "name": "   " })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["error"], "invalid_input",
        "clients branch on the code, not the message, got {body}"
    );
    assert!(
        body["message"].is_string(),
        "the message should still explain itself, got {body}"
    );

    let (_, list) = call(&pool, Method::GET, "/v1/projects", None).await;
    assert_eq!(list, json!([]), "a rejected create must not leave a row");
}

// ---------------------------------------------------------------------------
// read
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn get_project_by_id(pool: SqlitePool) {
    let created = create(&pool, "Alpha Project").await;
    let id = created["id"].as_str().expect("an id");

    let (status, body) = call(&pool, Method::GET, &format!("/v1/projects/{id}"), None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], created["id"]);
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn get_project_by_slug(pool: SqlitePool) {
    let created = create(&pool, "Alpha Project").await;

    let (status, body) = call(&pool, Method::GET, "/v1/projects/alpha-project", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["id"], created["id"],
        "the slug should resolve to the same row the id does"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn get_project_returns_404_for_missing(pool: SqlitePool) {
    let (status, body) = call(&pool, Method::GET, "/v1/projects/no-such-thing", None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("no-such-thing")),
        "the message should name what was not found, got {body}"
    );
}

// ---------------------------------------------------------------------------
// update
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn patch_status_updates_and_returns_project(pool: SqlitePool) {
    let created = create(&pool, "Alpha Project").await;
    let id = created["id"].as_str().expect("an id");

    let (status, body) = call(
        &pool,
        Method::PATCH,
        &format!("/v1/projects/{id}"),
        Some(json!({ "status": "active" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "active");
    assert_eq!(body["id"], created["id"], "the same project came back");

    // The response is the reloaded row, so it proves the write landed rather
    // than echoing the request body.
    let (_, fetched) = call(&pool, Method::GET, &format!("/v1/projects/{id}"), None).await;
    assert_eq!(fetched["status"], "active");
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn patch_status_rejects_invalid_status(pool: SqlitePool) {
    let created = create(&pool, "Alpha Project").await;
    let id = created["id"].as_str().expect("an id");

    let (status, body) = call(
        &pool,
        Method::PATCH,
        &format!("/v1/projects/{id}"),
        Some(json!({ "status": "nonsense" })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_input");

    let (_, fetched) = call(&pool, Method::GET, &format!("/v1/projects/{id}"), None).await;
    assert_eq!(
        fetched["status"], "recon",
        "a rejected transition must not change the stored status"
    );
}

// ---------------------------------------------------------------------------
// snippets
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_snippets_empty(pool: SqlitePool) {
    let (status, body) = call(&pool, Method::GET, "/v1/snippets?limit=10", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]), "empty snippet table is empty array");
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_snippets_returns_snippets(pool: SqlitePool) {
    // Create test data
    let conversation_id = uuid::Uuid::now_v7().to_string();
    let message_id = uuid::Uuid::now_v7().to_string();
    let snippet_id = uuid::Uuid::now_v7().to_string();

    // conversation.source_id references source(id) and nothing seeds it.
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES ('source-1', 'source-1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversation (id, source_id, title) VALUES (?, 'source-1', 'Test')")
        .bind(&conversation_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json) VALUES (?, ?, 'user', '{}')",
    )
    .bind(&message_id)
    .bind(&conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 0, 'code', 'test', 'hash')"
    )
    .bind(&snippet_id)
    .bind(&message_id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, body) = call(&pool, Method::GET, "/v1/snippets?limit=10", None).await;

    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().expect("array");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], snippet_id);
}

// ---------------------------------------------------------------------------
// cart
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn add_to_cart_creates_and_returns_item_ids(pool: SqlitePool) {
    let project = create(&pool, "Test Project").await;
    let project_id = project["id"].as_str().expect("id");

    // Create test snippet
    let conversation_id = uuid::Uuid::now_v7().to_string();
    let message_id = uuid::Uuid::now_v7().to_string();
    let snippet_id = uuid::Uuid::now_v7().to_string();

    // conversation.source_id references source(id) and nothing seeds it.
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES ('source-1', 'source-1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversation (id, source_id, title) VALUES (?, 'source-1', 'Test')")
        .bind(&conversation_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json) VALUES (?, ?, 'user', '{}')",
    )
    .bind(&message_id)
    .bind(&conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 0, 'code', 'test', 'hash')"
    )
    .bind(&snippet_id)
    .bind(&message_id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, body) = call(
        &pool,
        Method::POST,
        &format!("/v1/projects/{project_id}/cart"),
        Some(json!({ "snippet_ids": [snippet_id] })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let item_ids = body.as_array().expect("array");
    assert_eq!(item_ids.len(), 1);
    assert!(item_ids[0].is_string());
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_cart_returns_items(pool: SqlitePool) {
    let project = create(&pool, "Test Project").await;
    let project_id = project["id"].as_str().expect("id");

    // Create test snippet
    let conversation_id = uuid::Uuid::now_v7().to_string();
    let message_id = uuid::Uuid::now_v7().to_string();
    let snippet_id = uuid::Uuid::now_v7().to_string();

    // conversation.source_id references source(id) and nothing seeds it.
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES ('source-1', 'source-1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversation (id, source_id, title) VALUES (?, 'source-1', 'Test')")
        .bind(&conversation_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json) VALUES (?, ?, 'user', '{}')",
    )
    .bind(&message_id)
    .bind(&conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 0, 'code', 'test', 'hash')"
    )
    .bind(&snippet_id)
    .bind(&message_id)
    .execute(&pool)
    .await
    .unwrap();

    // Add to cart
    call(
        &pool,
        Method::POST,
        &format!("/v1/projects/{project_id}/cart"),
        Some(json!({ "snippet_ids": [snippet_id] })),
    )
    .await;

    let (status, body) = call(
        &pool,
        Method::GET,
        &format!("/v1/projects/{project_id}/cart"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = body.as_array().expect("array");
    assert_eq!(items.len(), 1);
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn remove_from_cart_deletes_item(pool: SqlitePool) {
    let project = create(&pool, "Test Project").await;
    let project_id = project["id"].as_str().expect("id");

    // Create test snippet
    let conversation_id = uuid::Uuid::now_v7().to_string();
    let message_id = uuid::Uuid::now_v7().to_string();
    let snippet_id = uuid::Uuid::now_v7().to_string();

    // conversation.source_id references source(id) and nothing seeds it.
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES ('source-1', 'source-1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversation (id, source_id, title) VALUES (?, 'source-1', 'Test')")
        .bind(&conversation_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json) VALUES (?, ?, 'user', '{}')",
    )
    .bind(&message_id)
    .bind(&conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 0, 'code', 'test', 'hash')"
    )
    .bind(&snippet_id)
    .bind(&message_id)
    .execute(&pool)
    .await
    .unwrap();

    // Add to cart
    let (status, body) = call(
        &pool,
        Method::POST,
        &format!("/v1/projects/{project_id}/cart"),
        Some(json!({ "snippet_ids": [snippet_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "adding to the cart creates it");
    let item_id = body.as_array().expect("array")[0]
        .as_str()
        .expect("item_id");

    // Remove from cart
    let (status, _) = call(
        &pool,
        Method::DELETE,
        &format!("/v1/projects/{project_id}/cart/{item_id}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify empty
    let (_, body) = call(
        &pool,
        Method::GET,
        &format!("/v1/projects/{project_id}/cart"),
        None,
    )
    .await;
    assert_eq!(body, json!([]));
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn add_to_cart_invalid_snippet_returns_404(pool: SqlitePool) {
    let project = create(&pool, "Test Project").await;
    let project_id = project["id"].as_str().expect("id");

    let (status, body) = call(
        &pool,
        Method::POST,
        &format!("/v1/projects/{project_id}/cart"),
        Some(json!({ "snippet_ids": ["nonexistent"] })),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_scored_snippets_returns_sorted_by_score(pool: SqlitePool) {
    let project = create(&pool, "Compiler Project").await;
    let project_id = project["id"].as_str().expect("id");

    let conversation_id = "conv-1";
    let message_id_1 = "msg-1";
    let message_id_2 = "msg-2";

    sqlx::query("INSERT INTO source (id, name) VALUES ('source-1', 'ChatGPT')")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO conversation (id, source_id, title) VALUES (?, 'source-1', 'Scored Test')",
    )
    .bind(conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, turn_index, content_json) VALUES (?, ?, 'user', 0, '{}')",
    )
    .bind(message_id_1)
    .bind(conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, turn_index, content_json) VALUES (?, ?, 'assistant', 1, '{}')",
    )
    .bind(message_id_2)
    .bind(conversation_id)
    .execute(&pool)
    .await
    .unwrap();

    // High scoring snippet: code + markers ("we decided", "required")
    let s1 = "snip-high";
    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 0, 'code', 'we decided the fix is required', 'hash-1')",
    )
    .bind(s1)
    .bind(message_id_2)
    .execute(&pool)
    .await
    .unwrap();

    // Medium scoring snippet: text + marker ("must")
    let s2 = "snip-mid";
    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 1, 'text', 'this must be done', 'hash-2')",
    )
    .bind(s2)
    .bind(message_id_1)
    .execute(&pool)
    .await
    .unwrap();

    // Low scoring snippet: plain text, no markers
    let s3 = "snip-low";
    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash) VALUES (?, ?, 2, 'text', 'plain words without any keywords', 'hash-3')",
    )
    .bind(s3)
    .bind(message_id_1)
    .execute(&pool)
    .await
    .unwrap();

    // Call GET /v1/projects/:id/snippets/scored
    let (status, body) = call(
        &pool,
        Method::GET,
        &format!("/v1/projects/{project_id}/snippets/scored"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let items = body.as_array().expect("array of scored snippets");
    assert_eq!(items.len(), 3);

    let score0 = items[0]["score"].as_f64().expect("score float");
    let score1 = items[1]["score"].as_f64().expect("score float");
    let score2 = items[2]["score"].as_f64().expect("score float");

    assert!(
        score0 >= score1 && score1 >= score2,
        "snippets should be sorted descending by score: {score0}, {score1}, {score2}"
    );
    assert_eq!(items[0]["id"], s1);
    assert_eq!(items[0]["score_details"]["code"], 1.0);

    // Verify 404 for missing project
    let (not_found_status, not_found_body) = call(
        &pool,
        Method::GET,
        "/v1/projects/nonexistent-project/snippets/scored",
        None,
    )
    .await;
    assert_eq!(not_found_status, StatusCode::NOT_FOUND);
    assert_eq!(not_found_body["error"], "not_found");
}
