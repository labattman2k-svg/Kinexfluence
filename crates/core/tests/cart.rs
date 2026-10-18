//! Cart service integration tests.
//!
//! These run against a real migrated database through `#[sqlx::test]`, because
//! what the cart does is mostly SQL: get-or-create an item, join it to a
//! project, and delete a membership that has to report whether it removed
//! anything.
//!
//! The seed helper builds the whole chain — source, conversation, message,
//! snippet — because every one of those is a foreign key the snippet depends
//! on. Rows invented in isolation would fail the same way a user's would not.

use kfx_core::cart::{self, CartError};
use kfx_core::project::{self, CreateProject};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Seed one source → conversation → message → snippet chain.
///
/// Returns the snippet id.
async fn seed_snippet(pool: &SqlitePool) -> Result<String, sqlx::Error> {
    // `conversation.source_id` references `source(id)`, and nothing seeds that
    // table, so it has to be created before any conversation can exist.
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES ('chatgpt', 'chatgpt')")
        .execute(pool)
        .await?;

    let conversation_id = Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO conversation (id, source_id, title) VALUES (?, 'chatgpt', 'Test')")
        .bind(&conversation_id)
        .execute(pool)
        .await?;

    let message_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO message (id, conversation_id, role, content_json)
         VALUES (?, ?, 'user', '[]')",
    )
    .bind(&message_id)
    .bind(&conversation_id)
    .execute(pool)
    .await?;

    let snippet_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO snippet (id, message_id, block_index, kind, text, hash)
         VALUES (?, ?, 0, 'code', 'fn main() {}', 'hash')",
    )
    .bind(&snippet_id)
    .bind(&message_id)
    .execute(pool)
    .await?;

    Ok(snippet_id)
}

async fn seed_project(pool: &SqlitePool, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let created = project::create(
        pool,
        CreateProject {
            name: name.to_owned(),
            ..CreateProject::default()
        },
    )
    .await?;
    Ok(created.id)
}

async fn cart_len(pool: &SqlitePool, project_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM item_project WHERE project_id = ? AND status = 'cart'")
        .bind(project_id)
        .fetch_one(pool)
        .await
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn add_to_cart_creates_item_and_entry(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let item_id = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;

    let item: Option<String> = sqlx::query_scalar("SELECT id FROM item WHERE id = ?")
        .bind(&item_id)
        .fetch_optional(&pool)
        .await?;
    assert!(
        item.is_some(),
        "adding to the cart should materialise an item row"
    );

    let item_for_snippet: Option<String> =
        sqlx::query_scalar("SELECT id FROM item WHERE snippet_id = ?")
            .bind(&snippet_id)
            .fetch_optional(&pool)
            .await?;
    assert_eq!(
        item_for_snippet.as_deref(),
        Some(item_id.as_str()),
        "the item should point back at the snippet it came from"
    );

    assert_eq!(cart_len(&pool, &project_id).await?, 1);
    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn add_to_cart_is_idempotent(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let first = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;
    let second = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;

    assert_eq!(
        first, second,
        "the same snippet should resolve to the same item, not a second one"
    );
    assert_eq!(
        cart_len(&pool, &project_id).await?,
        1,
        "adding twice must not create a second cart entry"
    );

    let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item")
        .fetch_one(&pool)
        .await?;
    assert_eq!(items, 1, "and must not create a second item row");

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn list_cart_returns_added_items(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let first = seed_snippet(&pool).await?;
    let second = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    cart::add_to_cart(&pool, &project_id, &first).await?;
    cart::add_to_cart(&pool, &project_id, &second).await?;

    let items = cart::list_cart(&pool, &project_id).await?;
    assert_eq!(items.len(), 2);

    // `added_at` has second resolution, so two additions in the same second
    // tie; compare as a set rather than relying on the order.
    let mut listed: Vec<&str> = items
        .iter()
        .filter_map(|item| item.snippet_id.as_deref())
        .collect();
    listed.sort_unstable();
    let mut expected = vec![first.as_str(), second.as_str()];
    expected.sort_unstable();
    assert_eq!(listed, expected);

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn remove_from_cart_deletes_the_entry(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let item_id = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;
    assert_eq!(cart_len(&pool, &project_id).await?, 1);

    cart::remove_from_cart(&pool, &project_id, &item_id).await?;
    assert_eq!(cart_len(&pool, &project_id).await?, 0);

    assert_eq!(
        cart::list_cart(&pool, &project_id).await?.len(),
        0,
        "a removed item should not appear in the list"
    );

    // The item row survives: it is the snippet's materialisation, not the
    // cart membership.
    let item: Option<String> = sqlx::query_scalar("SELECT id FROM item WHERE id = ?")
        .bind(&item_id)
        .fetch_optional(&pool)
        .await?;
    assert!(
        item.is_some(),
        "removing from the cart must not delete the item"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn add_to_cart_rejects_unknown_snippet(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let error = cart::add_to_cart(&pool, &project_id, "no-such-snippet")
        .await
        .expect_err("a snippet that does not exist cannot be added");

    assert!(
        matches!(error, CartError::SnippetNotFound(_)),
        "got {error:?}"
    );
    assert_eq!(cart_len(&pool, &project_id).await?, 0);
    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn add_to_cart_rejects_unknown_project(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;

    let error = cart::add_to_cart(&pool, "no-such-project", &snippet_id)
        .await
        .expect_err("a project that does not exist cannot hold a cart");

    assert!(
        matches!(error, CartError::ProjectNotFound(_)),
        "got {error:?}"
    );
    Ok(())
}
