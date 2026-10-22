//! Tests for the context pack exporter.
//!
//! A pack is what the user takes away, so what matters is not that the
//! Markdown is produced but that it is *right*: the committed items and only
//! the committed items, grouped so a reader can find them, and fenced so a
//! reader's Markdown renderer does not eat a line of code.
//!
//! The tests run against a real migrated database, because what is being
//! tested is how the exporter and SQLite behave together.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use kfx_core::exporter::{export_project, ExportError};
use kfx_core::server;
use sqlx::SqlitePool;
use tower::ServiceExt;

/// Create a project, with a brief if one is given.
async fn make_project(pool: &SqlitePool, name: &str, brief: Option<&str>) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO project (id, name, slug, brief) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(name.to_lowercase().replace(' ', "-"))
        .bind(brief)
        .execute(pool)
        .await
        .expect("the project should be inserted");
    id
}

/// Add an item to a project, in the given status.
///
/// The item is written straight into the table rather than through the cart.
/// The cart would also create a snippet, a message and a conversation, and
/// the exporter reads none of them: the language rides on the item's own
/// metadata, which is where the cart puts it.
async fn add_item(
    pool: &SqlitePool,
    project_id: &str,
    kind: &str,
    text: &str,
    language: Option<&str>,
    status: &str,
    order: i64,
) -> String {
    let item_id = uuid::Uuid::now_v7().to_string();
    let metadata_json = serde_json::json!({ "language": language }).to_string();
    sqlx::query("INSERT INTO item (id, kind, text, metadata_json) VALUES (?, ?, ?, ?)")
        .bind(&item_id)
        .bind(kind)
        .bind(text)
        .bind(&metadata_json)
        .execute(pool)
        .await
        .expect("the item should be inserted");

    sqlx::query(
        "INSERT INTO item_project (item_id, project_id, status, committed_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&item_id)
    .bind(project_id)
    .bind(status)
    .bind(format!("2026-01-01 00:00:{order:02}"))
    .execute(pool)
    .await
    .expect("the item should be added to the project");

    item_id
}

/// The pack of a project, exported.
async fn pack(pool: &SqlitePool, project_id: &str) -> String {
    export_project(pool, project_id)
        .await
        .expect("the pack should be exported")
}

/// One request through the router, returning the status, the content type, and
/// the body as text.
///
/// The export is Markdown, not JSON, so the JSON-decoding helper the other
/// API tests use cannot read it.
async fn call_raw(pool: &SqlitePool, uri: &str) -> (StatusCode, String, String) {
    let app = server::router(pool.clone(), None);
    let request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request should build");

    let response = app.oneshot(request).await.expect("router should respond");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should read")
        .to_bytes();

    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).expect("the body should be utf-8"),
    )
}

// ---------------------------------------------------------------------------
// What is in the pack
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn a_pack_names_the_project_and_carries_its_brief(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", Some("A project about storage.")).await;

    let pack = pack(&pool, &project).await;

    assert!(
        pack.starts_with("# Alpha\n\nA project about storage.\n\n"),
        "the pack should open with the project's name and brief: {pack}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn only_committed_items_are_in_the_pack(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", None).await;
    let decided = "We decided to use SQLite.";
    let candidate = "This one is still in the cart.";
    add_item(&pool, &project, "text", decided, None, "committed", 1).await;
    add_item(&pool, &project, "text", candidate, None, "cart", 2).await;

    let pack = pack(&pool, &project).await;

    assert!(
        pack.contains("We decided to use SQLite."),
        "a committed item belongs in the pack: {pack}"
    );
    assert!(
        !pack.contains("still in the cart"),
        "an item in the cart is a candidate, not context: {pack}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn a_pack_with_nothing_committed_says_so(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", None).await;

    let pack = pack(&pool, &project).await;

    assert!(
        pack.contains("_No items have been committed to this project yet._"),
        "an empty pack should say it is empty rather than look broken: {pack}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn a_project_that_does_not_exist_is_an_error(pool: SqlitePool) {
    let error = export_project(&pool, "no-such-project")
        .await
        .expect_err("a missing project should be an error");

    assert!(
        matches!(error, ExportError::ProjectNotFound(_)),
        "a missing project should report as not found, got {error:?}"
    );
}

// ---------------------------------------------------------------------------
// How it is grouped
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn prose_is_grouped_by_what_it_says(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", None).await;
    // Committed in the reverse of the reading order, so the sections have to
    // be put in order by the exporter and not by the order they arrived in.
    for (order, text) in [
        "The import takes about four seconds.",
        "Should the pack include the rejected options?",
        "The database must never be opened on a network share.",
        "We decided to use SQLite.",
    ]
    .iter()
    .enumerate()
    {
        let order = order as i64;
        add_item(&pool, &project, "text", text, None, "committed", order).await;
    }

    let pack = pack(&pool, &project).await;

    assert!(
        pack.contains("## Decisions\n\n- We decided to use SQLite."),
        "a decision belongs under Decisions: {pack}"
    );
    assert!(
        pack.contains("## Constraints\n\n- The database must never be opened"),
        "a constraint belongs under Constraints: {pack}"
    );
    assert!(
        pack.contains("## Open Questions\n\n- Should the pack include"),
        "a question belongs under Open Questions: {pack}"
    );
    assert!(
        pack.contains("## Notes\n\n- The import takes about four seconds."),
        "anything else is a note: {pack}"
    );

    let positions: Vec<usize> = [
        "## Decisions",
        "## Constraints",
        "## Open Questions",
        "## Notes",
    ]
    .iter()
    .map(|heading| {
        pack.find(heading)
            .unwrap_or_else(|| panic!("the pack should have a {heading} section: {pack}"))
    })
    .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the sections should be in reading order: {positions:?}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn a_fragment_that_runs_to_several_lines_stays_one_item(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", None).await;
    let fragment = "The first line.\nThe second line.";
    add_item(&pool, &project, "text", fragment, None, "committed", 1).await;

    let pack = pack(&pool, &project).await;

    assert!(
        pack.contains("- The first line.\n  The second line."),
        "a continuation line is indented, so the fragment stays one item: {pack}"
    );
}

// ---------------------------------------------------------------------------
// The code fence
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn code_is_fenced_with_its_language(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", None).await;
    let rust = "fn main() {}";
    let python = "print('hi')";
    add_item(&pool, &project, "code", rust, Some("rust"), "committed", 1).await;
    add_item(&pool, &project, "code", python, None, "committed", 2).await;

    let pack = pack(&pool, &project).await;

    assert!(
        pack.contains("## Code\n\n```rust\nfn main() {}\n```"),
        "code belongs in a fence that names its language: {pack}"
    );
    assert!(
        pack.contains("```\nprint('hi')\n```"),
        "a fence with no language is still a fence: {pack}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn code_containing_a_fence_is_fenced_longer(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", None).await;
    // A Markdown file quoted inside a code block. A fence of three backticks
    // would end at the first one, and the rest of the file would be read as
    // prose.
    let quoted = "```rust\nfn main() {}\n```";
    add_item(
        &pool,
        &project,
        "code",
        quoted,
        Some("markdown"),
        "committed",
        1,
    )
    .await;

    let pack = pack(&pool, &project).await;

    assert!(
        pack.contains("````markdown\n```rust\nfn main() {}\n```\n````"),
        "the fence has to be longer than the backticks inside it: {pack}"
    );
}

// ---------------------------------------------------------------------------
// The endpoint
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn the_export_endpoint_returns_markdown(pool: SqlitePool) {
    let project = make_project(&pool, "Alpha", Some("A project about storage.")).await;
    let decided = "We decided to use SQLite.";
    add_item(&pool, &project, "text", decided, None, "committed", 1).await;

    let (status, content_type, body) =
        call_raw(&pool, &format!("/v1/projects/{project}/export")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type, "text/markdown; charset=utf-8",
        "the endpoint serves a document, not a JSON envelope"
    );
    assert!(
        body.starts_with("# Alpha\n\nA project about storage.\n\n"),
        "the body is the pack: {body}"
    );
    assert!(
        body.contains("We decided to use SQLite."),
        "the body holds the committed items: {body}"
    );
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn the_export_endpoint_reports_an_unknown_project(pool: SqlitePool) {
    let (status, _content_type, _body) =
        call_raw(&pool, "/v1/projects/no-such-project/export").await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a project that does not exist is a 404, not an empty document"
    );
}
