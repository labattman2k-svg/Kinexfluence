//! Cart management service.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use crate::project::ACTOR_USER;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CartItem {
    pub item_id: String,
    pub snippet_id: Option<String>,
    pub kind: String,
    pub title: Option<String>,
    pub text: String,
    pub added_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CartError {
    #[error("Snippet not found: {0}")]
    SnippetNotFound(String),
    #[error("Project not found: {0}")]
    ProjectNotFound(String),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// The entity type recorded in `event_log` for a cart membership.
const ENTITY_ITEM_PROJECT: &str = "item_project";

/// The action recorded when a snippet joins a project's cart.
const ACTION_CREATED: &str = "created";

/// The action recorded when a snippet leaves a project's cart.
const ACTION_DELETED: &str = "deleted";

/// The action recorded when a cart membership becomes project context.
const ACTION_COMMITTED: &str = "committed";

/// Get or create an item from a snippet.
async fn get_or_create_item(pool: &SqlitePool, snippet_id: &str) -> Result<String, CartError> {
    let mut tx = pool.begin().await?;
    let item_id = item_for_snippet(&mut tx, snippet_id).await?;
    tx.commit().await?;
    Ok(item_id)
}

/// Get or create the item that holds a snippet's content.
///
/// An item is a snippet's materialisation: the cart and the probe engine
/// both need one, and there must be exactly one per snippet or a snippet
/// could be in two places at once. So the rule lives here and both callers
/// come to it.
///
/// The item is created whether or not anyone has asked for it yet. A probe
/// about a snippet that has never been in a cart is a probe about something
/// the user wants, and refusing it because the item does not exist would
/// refuse the exact case the mechanism exists for.
pub async fn item_for_snippet(
    tx: &mut Transaction<'_, Sqlite>,
    snippet_id: &str,
) -> Result<String, CartError> {
    // Try to find existing item
    let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM item WHERE snippet_id = ?")
        .bind(snippet_id)
        .fetch_optional(&mut **tx)
        .await?;

    if let Some((item_id,)) = existing {
        return Ok(item_id);
    }

    // Fetch snippet details
    let snippet: Option<(String, String, String, Option<String>)> =
        sqlx::query_as("SELECT id, kind, text, language FROM snippet WHERE id = ?")
            .bind(snippet_id)
            .fetch_optional(&mut **tx)
            .await?;

    // The snippet's own id is what the item points at, so it is not needed
    // here -- only its content is copied.
    let (_snippet_id, kind, text, language) =
        snippet.ok_or_else(|| CartError::SnippetNotFound(snippet_id.to_string()))?;

    // Create new item
    let item_id = Uuid::now_v7().to_string();
    let metadata_json = serde_json::json!({ "language": language }).to_string();

    sqlx::query(
        r#"
        INSERT INTO item (id, snippet_id, kind, text, metadata_json)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&item_id)
    .bind(snippet_id)
    .bind(&kind)
    .bind(&text)
    .bind(&metadata_json)
    .execute(&mut **tx)
    .await?;

    Ok(item_id)
}

/// Add a snippet to a project's cart.
pub async fn add_to_cart(
    pool: &SqlitePool,
    project_id: &str,
    snippet_id: &str,
) -> Result<String, CartError> {
    // Verify project exists
    let project_exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM project WHERE id = ?")
        .bind(project_id)
        .fetch_optional(pool)
        .await?;

    if project_exists.is_none() {
        return Err(CartError::ProjectNotFound(project_id.to_string()));
    }

    // Get or create item
    let item_id = get_or_create_item(pool, snippet_id).await?;

    // Check if already in cart (idempotent)
    let existing: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM item_project WHERE item_id = ? AND project_id = ? AND status = 'cart'",
    )
    .bind(&item_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Ok(item_id);
    }

    // Add to cart. The membership and the row that records it are written
    // together, so the log cannot claim a membership that is not there, and
    // the membership cannot appear without being logged.
    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO item_project (item_id, project_id, status, added_at)
        VALUES (?, ?, 'cart', ?)
        "#,
    )
    .bind(&item_id)
    .bind(project_id)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *tx)
    .await?;

    log_membership(&mut tx, &item_id, project_id, ACTION_CREATED).await?;

    tx.commit().await?;

    Ok(item_id)
}

/// List items in a project's cart.
pub async fn list_cart(pool: &SqlitePool, project_id: &str) -> Result<Vec<CartItem>, CartError> {
    let items = sqlx::query_as::<_, CartItem>(
        r#"
        SELECT
            ip.item_id,
            i.snippet_id,
            i.kind,
            i.title,
            i.text,
            ip.added_at
        FROM item_project ip
        JOIN item i ON ip.item_id = i.id
        WHERE ip.project_id = ? AND ip.status = 'cart'
        ORDER BY ip.added_at DESC
        "#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(items)
}

/// Remove an item from a project's cart.
pub async fn remove_from_cart(
    pool: &SqlitePool,
    project_id: &str,
    item_id: &str,
) -> Result<(), CartError> {
    // The delete and the row that records it are written together: the log
    // says the membership is gone, and it must not be able to say that while
    // the membership is still there.
    let mut tx = pool.begin().await?;

    let result = sqlx::query(
        "DELETE FROM item_project WHERE item_id = ? AND project_id = ? AND status = 'cart'",
    )
    .bind(item_id)
    .bind(project_id)
    .execute(&mut *tx)
    .await?;

    // Nothing was removed, so nothing happened and nothing is logged. The
    // transaction is dropped here, which rolls it back.
    if result.rows_affected() == 0 {
        return Err(CartError::Database(sqlx::Error::RowNotFound));
    }

    log_membership(&mut tx, item_id, project_id, ACTION_DELETED).await?;

    tx.commit().await?;

    Ok(())
}

/// Commit a project's cart: every item in it becomes project context.
///
/// An item in the cart is a candidate; a committed item is part of the
/// project's context, and the context pack is built from the committed ones.
/// This is the step that turns a list of possibilities into the project's
/// memory, so it is the one step the user takes deliberately.
///
/// Returns the number of items committed. Committing an empty cart is not an
/// error: it commits nothing, and says so.
pub async fn commit_cart(pool: &SqlitePool, project_id: &str) -> Result<usize, CartError> {
    // The project is checked the way `add_to_cart` checks it, so a commit for
    // a project that does not exist is an error rather than a silent zero.
    let project_exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM project WHERE id = ?")
        .bind(project_id)
        .fetch_optional(pool)
        .await?;

    if project_exists.is_none() {
        return Err(CartError::ProjectNotFound(project_id.to_string()));
    }

    let mut tx = pool.begin().await?;

    // The items are read before the update, because each one has to be
    // logged: the standard says a mutation of `item_project` must be
    // reconstructable.
    let item_ids: Vec<String> = sqlx::query_scalar(
        "SELECT item_id FROM item_project WHERE project_id = ? AND status = 'cart'",
    )
    .bind(project_id)
    .fetch_all(&mut *tx)
    .await?;

    for item_id in &item_ids {
        sqlx::query(
            "UPDATE item_project SET status = 'committed', committed_at = CURRENT_TIMESTAMP
             WHERE item_id = ? AND project_id = ?",
        )
        .bind(item_id)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;

        log_membership(&mut tx, item_id, project_id, ACTION_COMMITTED).await?;
    }

    tx.commit().await?;
    Ok(item_ids.len())
}

/// Write one `event_log` row for a change to a cart membership.
///
/// The standard names `item_project` among the entities whose mutations must
/// be reconstructable, and a membership changes in three ways: it is added,
/// it is removed, and it is committed into the project's context. All three
/// are written here, so the rule is kept in one place rather than repeated
/// at each call site.
async fn log_membership(
    tx: &mut Transaction<'_, Sqlite>,
    item_id: &str,
    project_id: &str,
    action: &str,
) -> Result<(), CartError> {
    sqlx::query(
        "INSERT INTO event_log (id, entity_type, entity_id, action, actor, payload_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(ENTITY_ITEM_PROJECT)
    .bind(item_id)
    .bind(action)
    .bind(ACTOR_USER)
    .bind(serde_json::json!({ "project_id": project_id }).to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
