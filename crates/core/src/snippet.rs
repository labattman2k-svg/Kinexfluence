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

/// A snippet row combined with message and conversation turn metadata.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SnippetWithMetadata {
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
    pub role: Option<String>,
    pub turn_index: Option<i32>,
    pub total_turns: Option<i32>,
}

impl SnippetWithMetadata {
    /// Convert into a base [`Snippet`].
    pub fn into_snippet(self) -> Snippet {
        Snippet {
            id: self.id,
            message_id: self.message_id,
            block_index: self.block_index,
            segment_index: self.segment_index,
            granularity: self.granularity,
            kind: self.kind,
            text: self.text,
            language: self.language,
            char_start: self.char_start,
            char_end: self.char_end,
            hash: self.hash,
            created_at: self.created_at,
        }
    }
}

/// List all snippets joined with message role, turn index, and conversation total turns.
pub async fn list_with_metadata(
    pool: &SqlitePool,
) -> Result<Vec<SnippetWithMetadata>, sqlx::Error> {
    let query = r#"
        SELECT
            s.id,
            s.message_id,
            s.block_index,
            s.segment_index,
            s.granularity,
            s.kind,
            s.text,
            s.language,
            s.char_start,
            s.char_end,
            s.hash,
            s.created_at,
            m.role,
            m.turn_index,
            CAST(c.total_turns AS INTEGER) AS total_turns
        FROM snippet s
        LEFT JOIN message m ON s.message_id = m.id
        LEFT JOIN (
            SELECT conversation_id, COUNT(*) AS total_turns
            FROM message
            GROUP BY conversation_id
        ) c ON m.conversation_id = c.conversation_id
        ORDER BY s.created_at DESC
    "#;

    sqlx::query_as::<_, SnippetWithMetadata>(query)
        .fetch_all(pool)
        .await
}
