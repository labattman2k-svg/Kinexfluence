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

/// The actions recorded in `event_log` for one item's membership of a
/// project, oldest first.
///
/// The order matters as much as the content: a cart that was filled and
/// emptied has to read back as `created` and then `deleted`, not as two
/// unordered rows.
async fn membership_actions(pool: &SqlitePool, item_id: &str) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT action FROM event_log
         WHERE entity_type = 'item_project' AND entity_id = ?
         ORDER BY rowid",
    )
    .bind(item_id)
    .fetch_all(pool)
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

    assert_eq!(
        membership_actions(&pool, &item_id).await?,
        vec!["created"],
        "adding to the cart is a mutation of item_project and must be logged"
    );
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

    assert_eq!(
        membership_actions(&pool, &first).await?,
        vec!["created"],
        "and must not log a second mutation for a membership that did not change"
    );

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

    assert_eq!(
        membership_actions(&pool, &item_id).await?,
        vec!["created", "deleted"],
        "the log should read as the membership being made and then unmade"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn removing_an_item_that_is_not_in_the_cart_logs_nothing(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let item_id = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;
    cart::remove_from_cart(&pool, &project_id, &item_id).await?;

    // A second removal changes nothing, so it must not claim that it did.
    let error = cart::remove_from_cart(&pool, &project_id, &item_id)
        .await
        .expect_err("removing an item that is not in the cart should fail");

    assert!(matches!(error, CartError::Database(_)), "got {error:?}");

    assert_eq!(
        membership_actions(&pool, &item_id).await?,
        vec!["created", "deleted"],
        "a removal that changed nothing must not be logged"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn cart_mutations_are_logged_against_the_membership(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let item_id = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;
    cart::remove_from_cart(&pool, &project_id, &item_id).await?;

    // The whole row, not just the action: the standard says the mutation
    // must be reconstructable, and that means knowing which entity it
    // happened to and who did it.
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT entity_type, action, actor FROM event_log
         WHERE entity_type = 'item_project' AND entity_id = ?
         ORDER BY rowid",
    )
    .bind(&item_id)
    .fetch_all(&pool)
    .await?;

    assert_eq!(rows.len(), 2, "both mutations should be logged: {rows:?}");
    for (entity_type, action, actor) in &rows {
        assert_eq!(entity_type, "item_project", "the entity is the membership");
        assert_eq!(actor, "user", "the cart is the user's to change");
        assert!(
            action == "created" || action == "deleted",
            "got an unexpected action: {action}"
        );
    }

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

// ---------------------------------------------------------------------------
// commit
// ---------------------------------------------------------------------------

/// The membership row for an item in a project: its status and when it was
/// committed.
async fn membership(
    pool: &SqlitePool,
    project_id: &str,
    item_id: &str,
) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT status, committed_at FROM item_project WHERE project_id = ? AND item_id = ?",
    )
    .bind(project_id)
    .bind(item_id)
    .fetch_optional(pool)
    .await
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn commit_cart_moves_the_cart_into_the_project(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = seed_snippet(&pool).await?;
    let second = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let first_item = cart::add_to_cart(&pool, &project_id, &first).await?;
    let second_item = cart::add_to_cart(&pool, &project_id, &second).await?;

    let committed = cart::commit_cart(&pool, &project_id).await?;

    assert_eq!(committed, 2, "both items should be committed");
    assert_eq!(
        cart_len(&pool, &project_id).await?,
        0,
        "a committed item is no longer in the cart"
    );
    assert_eq!(
        cart::list_cart(&pool, &project_id).await?.len(),
        0,
        "and it is no longer in the cart listing"
    );

    for item_id in [&first_item, &second_item] {
        let row = membership(&pool, &project_id, item_id).await?;
        assert_eq!(
            row.as_ref().map(|(status, _)| status.as_str()),
            Some("committed"),
            "the item's status should be committed"
        );
        assert!(
            row.as_ref().and_then(|(_, at)| at.as_ref()).is_some(),
            "a committed item should carry the moment it was committed"
        );
    }

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn commit_cart_leaves_already_committed_items_alone(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    cart::add_to_cart(&pool, &project_id, &snippet_id).await?;
    assert_eq!(cart::commit_cart(&pool, &project_id).await?, 1);

    // A second commit has nothing left to commit, and says so rather than
    // counting the same item twice.
    assert_eq!(
        cart::commit_cart(&pool, &project_id).await?,
        0,
        "committing twice should not commit the same item twice"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn commit_cart_writes_an_event_log_row(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snippet_id = seed_snippet(&pool).await?;
    let project_id = seed_project(&pool, "Alpha Project").await?;

    let item_id = cart::add_to_cart(&pool, &project_id, &snippet_id).await?;
    cart::commit_cart(&pool, &project_id).await?;

    let logged: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT entity_type, entity_id, action, actor
         FROM event_log
         WHERE entity_type = 'item_project' AND entity_id = ? AND action = 'committed'",
    )
    .bind(&item_id)
    .fetch_optional(&pool)
    .await?;

    let (entity_type, entity_id, action, actor) =
        logged.expect("a commit must leave a row in the event log");
    assert_eq!(entity_type, "item_project");
    assert_eq!(entity_id, item_id);
    assert_eq!(action, "committed");
    assert_eq!(actor, "user");

    assert_eq!(
        membership_actions(&pool, &item_id).await?,
        vec!["created", "committed"],
        "the whole history of the membership should read back in order"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn commit_cart_rejects_unknown_project(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let error = cart::commit_cart(&pool, "no-such-project")
        .await
        .expect_err("a project that does not exist cannot be committed to");

    assert!(
        matches!(error, CartError::ProjectNotFound(_)),
        "got {error:?}"
    );
    Ok(())
}
