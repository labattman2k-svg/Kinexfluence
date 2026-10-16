//! Snippet extraction: one canonical message becomes zero or more snippets.
//!
//! A snippet is the smallest unit the Atlas shows and the scorer ranks, so the
//! granularity decisions in this module are load-bearing for everything
//! downstream of it.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use tracing::warn;

use super::new_id;
use super::types::{CanonicalMessage, NewSnippet};

/// Granularity for a snippet produced by splitting a text block.
pub const GRANULARITY_PARAGRAPH: &str = "paragraph";
/// Granularity for a snippet that is a whole block.
pub const GRANULARITY_BLOCK: &str = "block";

/// Kind for prose.
pub const KIND_TEXT: &str = "text";
/// Kind for code.
pub const KIND_CODE: &str = "code";

/// Normalize text before hashing: trim, lowercase, and collapse interior
/// whitespace runs to a single space.
///
/// Public because the conversation content hash uses the same normalization.
/// Two different notions of "the same text" would be a quiet bug: a snippet
/// could be judged a duplicate of one conversation's text and not another's.
pub fn normalize_for_hash(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;

    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        for lower in ch.to_lowercase() {
            out.push(lower);
        }
    }

    out
}

/// Lowercase hex SHA-256 of the normalized text.
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize_for_hash(text).as_bytes());

    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing into a String cannot fail; the result is ignored rather than
        // unwrapped because `unwrap` is forbidden outside tests.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Extract snippets from one canonical message.
///
/// `message_id` is the row id the message will be given, since snippets
/// reference it. `segment_index` is a single counter across the whole message
/// rather than one per block, per the PR #3c brief.
///
/// A block type this does not recognise produces no snippet and a warning; it
/// never fails the import. The parser already routes unrecognised content into
/// `metadata_json.unknown_blocks`, so anything reaching here with an unknown
/// type came from a newer parser and should be seen rather than dropped
/// silently.
pub fn extract_snippets(message: &CanonicalMessage, message_id: &str) -> Vec<NewSnippet> {
    let mut snippets = Vec::new();
    let mut segment_index: i64 = 0;

    for (block_index, block) in message.blocks.iter().enumerate() {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match block_type {
            "text" => {
                let text = text_of(block);
                if text.is_empty() {
                    warn!(
                        target: "kfx.normalizer",
                        "text block has no usable text field; skipped"
                    );
                    continue;
                }

                for (paragraph, char_start, char_end) in paragraphs(text) {
                    let mut snippet = NewSnippet {
                        id: new_id(),
                        message_id: message_id.to_owned(),
                        block_index: block_index as i64,
                        segment_index,
                        granularity: GRANULARITY_PARAGRAPH.to_owned(),
                        kind: KIND_TEXT.to_owned(),
                        text: paragraph.to_owned(),
                        language: None,
                        char_start: Some(char_start),
                        char_end: Some(char_end),
                        hash: String::new(),
                    };
                    snippet.hash = content_hash(&snippet.text);
                    snippets.push(snippet);
                    segment_index += 1;
                }
            }

            "code" => {
                let text = text_of(block);
                if text.trim().is_empty() {
                    continue;
                }

                // Code stays whole. Splitting on blank lines would cut a
                // function in half, and a half-function is not a usable
                // fragment anywhere downstream.
                let mut snippet = NewSnippet {
                    id: new_id(),
                    message_id: message_id.to_owned(),
                    block_index: block_index as i64,
                    segment_index,
                    granularity: GRANULARITY_BLOCK.to_owned(),
                    kind: KIND_CODE.to_owned(),
                    text: text.to_owned(),
                    language: block
                        .get("language")
                        .and_then(Value::as_str)
                        .filter(|language| !language.is_empty())
                        .map(str::to_owned),
                    // The block is the unit, so there is no sub-block span.
                    char_start: None,
                    char_end: None,
                    hash: String::new(),
                };
                snippet.hash = content_hash(&snippet.text);
                snippets.push(snippet);
                segment_index += 1;
            }

            "execution_output" => {
                let text = text_of(block);
                if text.trim().is_empty() {
                    continue;
                }

                // Kind is text, not a third kind: the schema's `kind` column has
                // no established vocabulary beyond text and code, and
                // execution_output is prose as far as the scorer is concerned.
                let mut snippet = NewSnippet {
                    id: new_id(),
                    message_id: message_id.to_owned(),
                    block_index: block_index as i64,
                    segment_index,
                    granularity: GRANULARITY_BLOCK.to_owned(),
                    kind: KIND_TEXT.to_owned(),
                    text: text.to_owned(),
                    language: None,
                    // The block is the unit, so there is no sub-block span.
                    char_start: None,
                    char_end: None,
                    hash: String::new(),
                };
                snippet.hash = content_hash(&snippet.text);
                snippets.push(snippet);
                segment_index += 1;
            }

            other => {
                warn!(
                    target: "kfx.normalizer",
                    block_type = other,
                    "unrecognised content block; no snippet extracted"
                );
            }
        }
    }

    snippets
}

fn text_of(block: &Value) -> &str {
    block
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Split a text block on blank lines.
///
/// Returns non-empty paragraphs with their byte offsets in `text`. Empty and
/// whitespace-only segments are dropped: they carry nothing to show, and a
/// snippet of two newlines would be ranked like any other.
///
/// Offsets are bytes, not characters, because every consumer uses them to slice
/// the source string and Rust slices by byte. Character offsets would cost a
/// scan per read and would disagree with the slice the moment the text leaves
/// ASCII.
fn paragraphs(text: &str) -> Vec<(&str, i64, i64)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();

    let mut seg_start: usize = 0;
    let mut newline_run: usize = 0;

    for (byte_idx, ch) in text.char_indices() {
        if ch == '\n' {
            newline_run += 1;

            if newline_run == 2 {
                // The segment ends before the first newline of the run. `\n` is
                // one byte, so the previous byte is that newline.
                spans.push((seg_start, byte_idx - 1));
                seg_start = byte_idx + 1;
                newline_run = 0;
            }
            continue;
        }

        newline_run = 0;
    }

    spans.push((seg_start, text.len()));

    spans
        .into_iter()
        .filter_map(|(byte_start, byte_end)| trim_span(text, byte_start, byte_end))
        .collect()
}

/// Trim a span and shift its byte offsets to match.
fn trim_span(text: &str, byte_start: usize, byte_end: usize) -> Option<(&str, i64, i64)> {
    let raw = text.get(byte_start..byte_end)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let leading_bytes = raw.len() - raw.trim_start().len();
    let trailing_bytes = raw.trim_start().len() - trimmed.len();

    // `usize` to `i64` for the INTEGER columns. A single block would have to
    // exceed `i64::MAX` bytes for this to lose anything.
    Some((
        trimmed,
        (byte_start + leading_bytes) as i64,
        (byte_end - trailing_bytes) as i64,
    ))
}
