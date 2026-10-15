//! The provider-adapter contract: one implementation per source.

use serde::{Deserialize, Serialize};

/// A provider-neutral capture produced by an adapter.
///
/// `conversation`, `messages` and `attachments` hold the canonical JSON shape
/// pinned by the PR #3a brief (section 4). `crates/adapters/src/chatgpt`
/// builds that shape; PR #3b normalizes it into the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCapture {
    pub source: String,
    pub conversation: serde_json::Value,
    pub messages: Vec<serde_json::Value>,
    pub attachments: Vec<serde_json::Value>,
}

/// Why an export could not be parsed.
///
/// Adapters return this rather than `anyhow::Error` so callers can
/// distinguish "this is not an export at all" from "this export is
/// malformed at a known place" without parsing an error string.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// The bytes are not a recognised export of any supported shape.
    #[error("input is not a recognised export format")]
    InvalidFormat,

    /// The export declares a version this adapter does not implement.
    #[error("unsupported export version: {0}")]
    UnsupportedVersion(String),

    /// A conversation or message was structurally malformed.
    ///
    /// `index` is the position of the offending conversation in the export
    /// array, or of the offending message within its conversation when the
    /// fault is message-level.
    #[error("malformed message at index {index}: {reason}")]
    MalformedMessage { index: usize, reason: String },

    /// Reading the export failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Parses one provider's export format into a [`RawCapture`].
pub trait ProviderAdapter: Send + Sync {
    /// Stable identifier for this provider, e.g. `"chatgpt"`.
    fn id(&self) -> &'static str;

    /// Whether this adapter handles captures taken from `url`.
    fn matches_url(&self, url: &str) -> bool;

    /// Parse an export file into one capture per conversation.
    ///
    /// Returns a vector rather than a single capture because a real export
    /// holds many conversations; returning only the first would silently
    /// discard the rest.
    ///
    /// Implementations must not panic on untrusted input, and must not
    /// perform network access.
    fn parse_export(&self, bytes: &[u8]) -> Result<Vec<RawCapture>, AdapterError>;

    /// Export shapes this adapter accepts.
    ///
    /// Not every provider versions its export; an identifier for the shape
    /// is acceptable where no provider-declared version exists.
    fn supported_export_versions(&self) -> &[&'static str];
}
