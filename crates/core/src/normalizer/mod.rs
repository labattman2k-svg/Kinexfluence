//! Normalizer: `RawCapture` in, schema-shaped rows out.
//!
//! Pure transformation. No database writes, no CLI, no network. The output is
//! plain data mirroring the tables; PR #3d is what inserts it.
//!
//! Read `docs/adr/0007-import-scope-export-only.md` for the decisions this
//! implements: import is project-blind, attachments are references only, and
//! the conversation content hash is over normalized content rather than raw
//! file bytes.

pub mod snippets;
pub mod types;

use uuid::Uuid;

pub use snippets::{
    content_hash, extract_snippets, normalize_for_hash, GRANULARITY_BLOCK, GRANULARITY_PARAGRAPH,
    KIND_CODE, KIND_TEXT,
};
pub use types::{
    CanonicalBlock, CanonicalConversation, CanonicalMessage, NewAttachment, NewConversation,
    NewMessage, NewSnippet, NormalizeError, NormalizedBatch,
};

/// A new row id.
pub(crate) fn new_id() -> String {
    Uuid::now_v7().to_string()
}
