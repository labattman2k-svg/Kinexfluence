//! Serde types for the ChatGPT export file, and the canonical capture shape.
//!
//! Two shapes live here, and they are deliberately separate:
//!
//! * `Raw*` mirrors the provider's `conversations.json` as it actually is.
//!   These types only read; unknown fields are ignored so a provider adding
//!   metadata cannot break the parse.
//! * `Capture*` is our canonical output, pinned by the PR #3a brief
//!   (section 4). It is the contract PR #3b normalizes into the database.

use serde::{Deserialize, Serialize};
use serde_json::{to_value, Value};
use std::collections::HashMap;

use crate::provider::{AdapterError, RawCapture};

// ---------------------------------------------------------------------------
// Raw export shape
// ---------------------------------------------------------------------------

/// One element of the provider's top-level `conversations.json` array.
#[derive(Debug, Deserialize)]
pub struct RawConversation {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Kept as raw JSON so the value round-trips exactly as exported.
    #[serde(default)]
    pub create_time: Value,
    #[serde(default)]
    pub update_time: Value,
    /// ChatGPT records the model here rather than in a top-level `model`.
    #[serde(default)]
    pub default_model_slug: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mapping: Option<HashMap<String, RawNode>>,
    #[serde(default)]
    pub current_node: Option<String>,
}

/// A node in the conversation tree. `message` is `null` for structural
/// placeholders, which is legitimate and must not be treated as an error.
#[derive(Debug, Deserialize)]
pub struct RawNode {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
    #[serde(default)]
    pub message: Option<RawMessage>,
}

#[derive(Debug, Deserialize)]
pub struct RawMessage {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub author: Option<RawAuthor>,
    /// Kept as raw JSON rather than a typed struct so an unrecognised
    /// `content` can be preserved into `metadata_json.unknown_blocks`
    /// verbatim, with none of its fields dropped.
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub create_time: Value,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct RawAuthor {
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawContent {
    #[serde(default)]
    pub content_type: Option<String>,
    /// `parts` is loosely typed on purpose: it holds strings for prose and
    /// structured objects for other content types.
    #[serde(default)]
    pub parts: Vec<Value>,
}

/// An attachment reference as it appears in a message's metadata.
#[derive(Debug, Deserialize)]
pub struct RawAttachment {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
}

// ---------------------------------------------------------------------------
// Canonical capture shape (the PR #3a contract)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureConversation {
    pub external_id: String,
    pub title: Option<String>,
    /// Unix epoch seconds, verbatim from the export. Deliberately untyped:
    /// converting to `DateTime` is PR #3b's job, when it satisfies
    /// `message.created_at DATETIME`.
    pub create_time: Value,
    pub update_time: Value,
    pub model: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    ExecutionOutput {
        text: String,
    },
}

/// Raw blocks this parser did not understand, preserved verbatim so nothing
/// is silently lost and PR #3b can revisit them.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureMetadata {
    pub unknown_blocks: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureMessage {
    pub external_id: String,
    pub parent_external_id: Option<String>,
    pub role: String,
    /// Position in reconstructed chronological order, counting only messages
    /// on the active path. Starts at 0.
    pub turn_index: usize,
    pub create_time: Value,
    pub blocks: Vec<ContentBlock>,
    /// Always present, `null` when there was nothing unknown.
    pub metadata_json: Option<CaptureMetadata>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureAttachment {
    pub external_id: String,
    pub filename: Option<String>,
    pub mime: Option<String>,
    /// Always `"missing"` in this phase: references are recorded, never
    /// fetched (ADR 0007).
    pub status: String,
}

/// The canonical capture, before conversion into the adapter's
/// [`RawCapture`] envelope.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CanonicalCapture {
    pub source: String,
    pub conversation: CaptureConversation,
    pub messages: Vec<CaptureMessage>,
    pub attachments: Vec<CaptureAttachment>,
}

impl CanonicalCapture {
    /// Wrap the canonical shape in the adapter envelope.
    ///
    /// These types contain only JSON-representable values, so serialization
    /// cannot fail in practice; the error is mapped rather than unwrapped
    /// because `unwrap` is forbidden outside tests.
    pub fn into_raw(self) -> Result<RawCapture, AdapterError> {
        let CanonicalCapture {
            source,
            conversation,
            messages,
            attachments,
        } = self;

        let bad = |_| AdapterError::InvalidFormat;

        Ok(RawCapture {
            source,
            conversation: to_value(conversation).map_err(bad)?,
            messages: messages
                .into_iter()
                .map(to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(bad)?,
            attachments: attachments
                .into_iter()
                .map(to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(bad)?,
        })
    }
}
