//! Writing a normalized batch to the database.
//!
//! The normalizer decides what the rows are; this module decides whether they
//! land. Two things live here and nowhere else: the transaction boundary, and
//! the rule for what happens on a re-import.
//!
//! Nothing in this module decides *content*. No hashing, no parsing, no
//! timestamp conversion — a caller can read every insert below and know
//! exactly what reaches the schema.

use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::normalizer::{NewAttachment, NewConversation, NewMessage, NewSnippet, NormalizedBatch};

/// `attachment.status` for a reference whose file is not in the export.
///
/// Migration 0002's CHECK constraint is satisfied by any status that is not
/// `present`, including this one. See [`crate::normalizer`] for where the
/// value is decided.
const STATUS_MISSING: &str = "missing";

/// Why a batch could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The database refused an insert, or a transaction could not be opened
    /// or committed. Constraint violations arrive here: `sqlx::Error` carries
    /// the SQLite result code and message.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    /// Reserved for id generation failures. Currently unconstructible because
    /// `normalize()` assigns every row id before the batch reaches the writer.
    /// Kept so the public API remains stable if id generation ever moves here.
    #[error("id generation failed: {0}")]
    Id(String),

    /// The batch referred to a row that is not in the batch.
    ///
    /// Without this check such a row would be silently dropped and the summary
    /// would report a smaller import than the caller handed over, which is
    /// worse than refusing it.
    #[error("batch invariant violated: {0}")]
    Invariant(String),
}

/// What one [`write_batch`] call wrote, and what it deliberately did not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Conversations written.
    pub conversations_inserted: usize,
    /// Conversations left alone because their content was already present.
    pub conversations_skipped: usize,
    /// Message rows written.
    pub messages_inserted: usize,
    /// Snippet rows written.
    pub snippets_inserted: usize,
    /// Attachment rows written, present and missing alike.
    ///
    /// Every attachment in a batch becomes a row; `status` records whether the
    /// file came with it. Use [`ImportSummary::attachments_present`] for the
    /// count of rows whose file is actually there.
    pub attachments_inserted: usize,
    /// Attachment rows whose file is not in the export.
    pub attachments_missing: usize,
}

impl ImportSummary {
    /// Attachment rows written whose file is present.
    ///
    /// A ChatGPT export carries references, not files, so this is zero for
    /// every import today.
    pub fn attachments_present(&self) -> usize {
        self.attachments_inserted - self.attachments_missing
    }
}

/// Write a batch of normalized rows.
///
/// The whole batch runs in one transaction. Either every row lands or none
/// does, so a batch that violates a constraint leaves the database exactly as
/// it was — there is no state in which a conversation exists without the
/// messages that belong to it.
///
/// # Ordering
///
/// Within a conversation the inserts run conversation, then messages, then
/// snippets, then attachments, because each level references the one before
/// it.
///
/// # Re-import
///
/// A conversation whose `content_hash` is already in the database is skipped
/// whole -- see [`already_imported`].
pub async fn write_batch(
    pool: &SqlitePool,
    batch: NormalizedBatch,
) -> Result<ImportSummary, WriteError> {
    check_invariants(&batch)?;

    let mut summary = ImportSummary::default();
    let mut tx = pool.begin().await?;

    for conversation in &batch.conversations {
        if already_imported(&mut tx, conversation).await? {
            summary.conversations_skipped += 1;
            continue;
        }

        // `conversation.source_id` references `source(id)` and nothing seeds
        // that table, so the first import from a provider has to create the
        // row. Without this the insert below fails the foreign key.
        ensure_source(&mut tx, &conversation.source_id).await?;

        insert_conversation(&mut tx, conversation).await?;
        summary.conversations_inserted += 1;

        let messages: Vec<&NewMessage> = batch
            .messages
            .iter()
            .filter(|message| message.conversation_id == conversation.id)
            .collect();

        for message in &messages {
            insert_message(&mut tx, message).await?;
            summary.messages_inserted += 1;
        }

        let snippets = batch
            .snippets
            .iter()
            .filter(|snippet| messages.iter().any(|m| m.id == snippet.message_id));

        for snippet in snippets {
            insert_snippet(&mut tx, snippet).await?;
            summary.snippets_inserted += 1;
        }

        let attachments = batch
            .attachments
            .iter()
            .filter(|attachment| attachment.owner_id == conversation.id);

        for attachment in attachments {
            insert_attachment(&mut tx, attachment).await?;
            summary.attachments_inserted += 1;
            if attachment.status == STATUS_MISSING {
                summary.attachments_missing += 1;
            }
        }
    }

    tx.commit().await?;
    Ok(summary)
}

/// Has this conversation's content been imported already?
///
/// A conversation with no `content_hash` cannot be recognised on a re-import,
/// so it is always inserted. That is the honest answer: with nothing to
/// compare there is no basis for calling it a duplicate, and falling back to
/// the external id would treat two genuinely different exports of the same
/// conversation as the same.
///
/// The whole conversation is skipped rather than just its row, and that is the
/// point: messages and snippets are keyed by freshly generated ids, so a
/// second run would produce new ids for identical content and nothing else
/// would stop the rows from doubling.
async fn already_imported(
    tx: &mut Transaction<'_, Sqlite>,
    conversation: &NewConversation,
) -> Result<bool, WriteError> {
    let Some(hash) = conversation.content_hash.as_deref() else {
        return Ok(false);
    };

    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM conversation WHERE content_hash = ? LIMIT 1")
            .bind(hash)
            .fetch_optional(&mut **tx)
            .await?;

    Ok(existing.is_some())
}

/// Reject a batch whose rows point at rows the batch does not contain.
fn check_invariants(batch: &NormalizedBatch) -> Result<(), WriteError> {
    for message in &batch.messages {
        if !contains_conversation(batch, &message.conversation_id) {
            return Err(WriteError::Invariant(format!(
                "message {} belongs to conversation {}, which is not in the batch",
                message.id, message.conversation_id
            )));
        }
    }

    for snippet in &batch.snippets {
        if !batch.messages.iter().any(|m| m.id == snippet.message_id) {
            return Err(WriteError::Invariant(format!(
                "snippet {} belongs to message {}, which is not in the batch",
                snippet.id, snippet.message_id
            )));
        }
    }

    for attachment in &batch.attachments {
        if !contains_conversation(batch, &attachment.owner_id) {
            return Err(WriteError::Invariant(format!(
                "attachment {} is owned by {}, which is not in the batch",
                attachment.id, attachment.owner_id
            )));
        }
    }

    Ok(())
}

fn contains_conversation(batch: &NormalizedBatch, id: &str) -> bool {
    batch.conversations.iter().any(|c| c.id == id)
}

async fn ensure_source(
    tx: &mut Transaction<'_, Sqlite>,
    source_id: &str,
) -> Result<(), WriteError> {
    sqlx::query("INSERT OR IGNORE INTO source (id, name) VALUES (?, ?)")
        .bind(source_id)
        .bind(source_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn insert_conversation(
    tx: &mut Transaction<'_, Sqlite>,
    conversation: &NewConversation,
) -> Result<(), WriteError> {
    sqlx::query(
        "INSERT INTO conversation (
             id, source_id, external_id, title, url, model,
             created_at, updated_at, raw_path, metadata_json, content_hash
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&conversation.id)
    .bind(&conversation.source_id)
    .bind(&conversation.external_id)
    .bind(&conversation.title)
    .bind(&conversation.url)
    .bind(&conversation.model)
    .bind(&conversation.created_at)
    .bind(&conversation.updated_at)
    .bind(&conversation.raw_path)
    .bind(&conversation.metadata_json)
    .bind(&conversation.content_hash)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_message(
    tx: &mut Transaction<'_, Sqlite>,
    message: &NewMessage,
) -> Result<(), WriteError> {
    sqlx::query(
        "INSERT INTO message (
             id, conversation_id, parent_id, role, turn_index,
             created_at, content_json, raw_json
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&message.id)
    .bind(&message.conversation_id)
    .bind(&message.parent_id)
    .bind(&message.role)
    .bind(message.turn_index)
    .bind(&message.created_at)
    .bind(&message.content_json)
    .bind(&message.raw_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_snippet(
    tx: &mut Transaction<'_, Sqlite>,
    snippet: &NewSnippet,
) -> Result<(), WriteError> {
    sqlx::query(
        "INSERT INTO snippet (
             id, message_id, block_index, segment_index, granularity, kind,
             text, language, char_start, char_end, hash
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&snippet.id)
    .bind(&snippet.message_id)
    .bind(snippet.block_index)
    .bind(snippet.segment_index)
    .bind(&snippet.granularity)
    .bind(&snippet.kind)
    .bind(&snippet.text)
    .bind(&snippet.language)
    .bind(snippet.char_start)
    .bind(snippet.char_end)
    .bind(&snippet.hash)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_attachment(
    tx: &mut Transaction<'_, Sqlite>,
    attachment: &NewAttachment,
) -> Result<(), WriteError> {
    sqlx::query(
        "INSERT INTO attachment (
             id, owner_type, owner_id, filename, mime, status, hash, blob_path
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&attachment.id)
    .bind(&attachment.owner_type)
    .bind(&attachment.owner_id)
    .bind(&attachment.filename)
    .bind(&attachment.mime)
    .bind(&attachment.status)
    .bind(&attachment.hash)
    .bind(&attachment.blob_path)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
