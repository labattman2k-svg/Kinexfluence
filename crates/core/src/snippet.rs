//! Snippet browsing service.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Snippet {
    pub id: String,
    pub message_id: String,
    pub block_index: i32,
    pub segment_index: i32,
    pub granularity: String,
    pub kind: String,
    pub text: String,
    pub language: Option<String>,
    pub char_start: Option<i32>,
    pub char_end: Option<i32>,
    pub hash: String,
    pub created_at: String,
}

/// List snippets with pagination.
pub async fn list(
    pool: &SqlitePool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Snippet>, sqlx::Error> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let snippets = sqlx::query_as::<_, Snippet>(
        r#"
        SELECT id, message_id, block_index, segment_index, granularity,
               kind, text, language, char_start, char_end, hash, created_at
        FROM snippet
        ORDER BY created_at DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(snippets)
}
