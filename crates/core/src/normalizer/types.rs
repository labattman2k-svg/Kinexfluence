//! Normalizer target types.
//!
//! These mirror the schema tables but carry no sqlx derives: the normalizer is
//! pure transformation, and PR #3d is what turns these into database rows.
//!
//! Columns the database fills in for itself are deliberately absent —
//! `conversation.imported_at`, `message.is_active_branch` and
//! `snippet.created_at` all have DEFAULTs. A normalizer that supplied them
//! would duplicate the schema and make its output depend on the clock.

use serde::Deserialize;
use serde_json::Value;

/// Why normalization could not complete.
#[derive(Debug, thiserror::Error)]
pub enum NormalizeError {
    /// Reading or writing a file failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The input was well-formed JSON but not a shape we can normalize.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A bug in the normalizer, not a problem with the input.
    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Views of the parser's canonical output
// ---------------------------------------------------------------------------

/// One element of a canonical message's `blocks` array.
///
/// Kept as JSON rather than a typed enum so an unrecognised block type can be
/// named in a warning and skipped, instead of failing the deserialize and
/// taking the whole import with it.
pub type CanonicalBlock = Value;

/// The parts of a canonical message the normalizer reads.
///
/// `crates/adapters` builds these objects; see section 4 of the PR #3a brief
/// for the shape. Deserializing into this struct rather than indexing into
/// `Value` at each use keeps the field names in one place.
#[derive(Debug, Clone, Deserialize)]
pub struct CanonicalMessage {
    /// The provider's node id for this message.
    #[serde(default)]
    pub external_id: Option<String>,
    /// The provider's node id for this message's parent.
    #[serde(default)]
    pub parent_external_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub turn_index: Option<i64>,
    /// Unix epoch seconds, verbatim, or null.
    #[serde(default)]
    pub create_time: Value,
    #[serde(default)]
    pub blocks: Vec<CanonicalBlock>,
    /// The verbatim source mapping node, for `message.raw_json`.
    #[serde(default)]
    pub raw: Value,
}

/// The parts of a canonical conversation the normalizer reads.
#[derive(Debug, Clone, Deserialize)]
pub struct CanonicalConversation {
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Unix epoch seconds, verbatim, or null.
    #[serde(default)]
    pub create_time: Value,
    /// Unix epoch seconds, verbatim, or null.
    #[serde(default)]
    pub update_time: Value,
}

// ---------------------------------------------------------------------------
// Target rows
// ---------------------------------------------------------------------------

/// A row destined for `conversation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewConversation {
    pub id: String,
    /// Matches `source.id`. The caller is responsible for that row existing.
    pub source_id: String,
    /// The provider's conversation id.
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub model: Option<String>,
    /// RFC 3339 UTC, converted from the export's unix epoch seconds.
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    /// Filesystem path of the archived raw export, when there is one.
    pub raw_path: Option<String>,
    pub metadata_json: Option<String>,
    /// Hash over the normalized message sequence, for re-import detection.
    /// See `docs/adr/0007-import-scope-export-only.md`.
    pub content_hash: Option<String>,
}

/// A row destined for `message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    pub id: String,
    pub conversation_id: String,
    /// Resolved from the provider's `parent_external_id` to a `message.id`.
    /// `None` when the parent is not on the captured path.
    pub parent_id: Option<String>,
    pub role: String,
    pub turn_index: i64,
    pub created_at: Option<String>,
    /// The canonical block array, serialized.
    pub content_json: String,
    /// The verbatim source node, serialized.
    pub raw_json: Option<String>,
}

/// A row destined for `snippet`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSnippet {
    pub id: String,
    pub message_id: String,
    /// Index of the source block within its message.
    pub block_index: i64,
    /// Position of this snippet in the message's extracted sequence, from 0.
    pub segment_index: i64,
    /// `paragraph` or `block`.
    pub granularity: String,
    /// `text` or `code`.
    pub kind: String,
    pub text: String,
    /// Set for code snippets when the export declared a language.
    pub language: Option<String>,
    /// Byte offsets within the source block's text, UTF-8 encoded.
    /// `None` for block-granularity snippets where the block is the unit.
    ///
    /// The field is named for the `char_start` column it maps to; the unit is
    /// bytes, which is what a reader slicing the source text actually needs.
    pub char_start: Option<i64>,
    /// Exclusive byte offset within the source block's text, UTF-8 encoded.
    /// `None` for block-granularity snippets where the block is the unit.
    pub char_end: Option<i64>,
    /// SHA-256 over the normalized text; see [`crate::normalizer::snippets`].
    pub hash: String,
}

/// A row destined for `attachment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAttachment {
    pub id: String,
    /// Always `conversation` for now: captures attach to the conversation.
    pub owner_type: String,
    pub owner_id: String,
    pub filename: Option<String>,
    pub mime: Option<String>,
    /// `present` or `missing`, matching the constraint added by migration 0002.
    pub status: String,
    pub hash: Option<String>,
    pub blob_path: Option<String>,
}

/// Everything one `normalize` call produced, grouped by destination table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizedBatch {
    pub conversations: Vec<NewConversation>,
    pub messages: Vec<NewMessage>,
    pub snippets: Vec<NewSnippet>,
    pub attachments: Vec<NewAttachment>,
}
