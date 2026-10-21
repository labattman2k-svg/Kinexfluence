//! Cart management service.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

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

    // Add to cart
    sqlx::query(
        r#"
        INSERT INTO item_project (item_id, project_id, status, added_at)
        VALUES (?, ?, 'cart', ?)
        "#,
    )
    .bind(&item_id)
    .bind(project_id)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;

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
    let result = sqlx::query(
        "DELETE FROM item_project WHERE item_id = ? AND project_id = ? AND status = 'cart'",
    )
    .bind(item_id)
    .bind(project_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(CartError::Database(sqlx::Error::RowNotFound));
    }

    Ok(())
}
