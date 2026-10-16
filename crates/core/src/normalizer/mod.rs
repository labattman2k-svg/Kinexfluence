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

use chrono::{TimeZone, Utc};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use kfx_adapters::RawCapture;

pub use snippets::{
    content_hash, extract_snippets, normalize_for_hash, GRANULARITY_BLOCK, GRANULARITY_PARAGRAPH,
    KIND_CODE, KIND_TEXT,
};
pub use types::{
    CanonicalBlock, CanonicalConversation, CanonicalMessage, NewAttachment, NewConversation,
    NewMessage, NewSnippet, NormalizeError, NormalizedBatch,
};

/// Attachments are recorded against the conversation, not a message: the
/// export's references carry no reliable message association, and ADR 0007
/// makes them references only.
const OWNER_TYPE_CONVERSATION: &str = "conversation";

/// Status meaning "referenced but not captured".
const STATUS_MISSING: &str = "missing";
/// Status meaning "we hold the bytes".
const STATUS_PRESENT: &str = "present";

/// A new row id.
pub(crate) fn new_id() -> String {
    Uuid::now_v7().to_string()
}

/// Normalize every capture into rows for `conversation`, `message`, `snippet`
/// and `attachment`.
///
/// Captures are processed in the order given, and messages within a capture in
/// the order the parser emitted them, so the output is deterministic apart
/// from the generated ids.
pub fn normalize(captures: &[RawCapture]) -> Result<NormalizedBatch, NormalizeError> {
    let mut batch = NormalizedBatch::default();

    for (capture_index, capture) in captures.iter().enumerate() {
        normalize_capture(capture_index, capture, &mut batch)?;
    }

    Ok(batch)
}

fn normalize_capture(
    capture_index: usize,
    capture: &RawCapture,
    batch: &mut NormalizedBatch,
) -> Result<(), NormalizeError> {
    let conversation: CanonicalConversation = serde_json::from_value(capture.conversation.clone())
        .map_err(|error| {
            NormalizeError::InvalidInput(format!("capture {capture_index}: conversation: {error}"))
        })?;

    let messages: Vec<CanonicalMessage> = capture
        .messages
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            serde_json::from_value(raw.clone()).map_err(|error| {
                NormalizeError::InvalidInput(format!(
                    "capture {capture_index}: message {index}: {error}"
                ))
            })
        })
        .collect::<Result<_, _>>()?;

    let conversation_id = new_id();

    // Ids are assigned up front so a message's `parent_id` can be resolved
    // against its parent's id in a second pass. The provider's own node ids are
    // not ours to reuse, and `message` has no column to keep them in -- they
    // survive inside `raw_json`.
    let message_ids: Vec<String> = messages.iter().map(|_| new_id()).collect();
    let mut id_by_external: HashMap<&str, &str> = HashMap::new();
    for (message, id) in messages.iter().zip(&message_ids) {
        if let Some(external) = message.external_id.as_deref() {
            id_by_external.insert(external, id);
        }
    }

    batch.conversations.push(NewConversation {
        id: conversation_id.clone(),
        // `source.id` is the provider name; PR #3d owns creating that row.
        source_id: capture.source.clone(),
        external_id: conversation.external_id.clone(),
        title: conversation.title.clone(),
        url: conversation.url.clone(),
        model: conversation.model.clone(),
        created_at: epoch_to_rfc3339(&conversation.create_time),
        updated_at: epoch_to_rfc3339(&conversation.update_time),
        raw_path: None,
        metadata_json: None,
        content_hash: Some(content_hash(&conversation_hash_source(&messages))),
    });

    for (index, (message, message_id)) in messages.iter().zip(&message_ids).enumerate() {
        let parent_id = message
            .parent_external_id
            .as_deref()
            .and_then(|external| id_by_external.get(external))
            .map(|id| (*id).to_owned());

        batch.messages.push(NewMessage {
            id: message_id.clone(),
            conversation_id: conversation_id.clone(),
            parent_id,
            role: message.role.clone().unwrap_or_else(|| "unknown".to_owned()),
            // The parser's turn_index is authoritative; the enumeration index
            // is only a fallback for input that predates it.
            turn_index: message.turn_index.unwrap_or(index as i64),
            created_at: epoch_to_rfc3339(&message.create_time),
            content_json: serde_json::to_string(&message.blocks)
                .map_err(|error| NormalizeError::Internal(format!("content_json: {error}")))?,
            raw_json: if message.raw.is_null() {
                None
            } else {
                Some(
                    serde_json::to_string(&message.raw)
                        .map_err(|error| NormalizeError::Internal(format!("raw_json: {error}")))?,
                )
            },
        });

        batch.snippets.extend(extract_snippets(message, message_id));
    }

    batch.attachments.extend(
        capture
            .attachments
            .iter()
            .filter_map(|raw| attachment_from(raw, &conversation_id)),
    );

    Ok(())
}

/// Build an attachment row from one capture attachment reference.
///
/// `None` when the reference has no id, since an unidentifiable attachment
/// cannot be linked to anything later.
fn attachment_from(raw: &Value, conversation_id: &str) -> Option<NewAttachment> {
    let external_id = raw.get("external_id").and_then(Value::as_str)?;
    if external_id.is_empty() {
        return None;
    }

    let hash = raw
        .get("hash")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let blob_path = raw
        .get("blob_path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    // Migration 0002's CHECK constraint requires exactly this correspondence:
    // anything not fully present is missing.
    let status = if hash.is_some() && blob_path.is_some() {
        STATUS_PRESENT
    } else {
        STATUS_MISSING
    };

    Some(NewAttachment {
        id: new_id(),
        owner_type: OWNER_TYPE_CONVERSATION.to_owned(),
        owner_id: conversation_id.to_owned(),
        filename: raw
            .get("filename")
            .and_then(Value::as_str)
            .map(str::to_owned),
        mime: raw.get("mime").and_then(Value::as_str).map(str::to_owned),
        status: status.to_owned(),
        hash,
        blob_path,
    })
}

/// The text the conversation content hash is taken over.
///
/// Roles and block texts in order, so that any change to what the user would
/// actually read changes the hash. Metadata the provider varies between
/// exports -- timestamps, the export filename, node ids -- is deliberately
/// excluded, which is the whole point: a re-export of the same conversation
/// should hash the same and be recognised as a duplicate, while a genuinely
/// edited conversation should not.
fn conversation_hash_source(messages: &[CanonicalMessage]) -> String {
    let mut out = String::new();

    for message in messages {
        out.push_str(message.role.as_deref().unwrap_or("unknown"));
        out.push('\n');
        for block in &message.blocks {
            out.push_str(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            out.push('\n');
        }
        out.push('\n');
    }

    out
}

/// Convert the export's unix epoch seconds into an RFC 3339 UTC timestamp.
///
/// Returns `None` for null, non-numeric or unrepresentable values rather than
/// inventing an epoch. The parser leaves these verbatim on purpose so that the
/// conversion happens exactly once, here, where the target column is known.
fn epoch_to_rfc3339(value: &Value) -> Option<String> {
    let seconds = value.as_f64()?;
    if !seconds.is_finite() {
        return None;
    }

    let mut whole = seconds.trunc() as i64;
    let mut nanos = ((seconds - seconds.trunc()) * 1_000_000_000.0).round() as i64;

    if nanos >= 1_000_000_000 {
        whole += 1;
        nanos -= 1_000_000_000;
    }
    if nanos < 0 {
        return None;
    }

    Utc.timestamp_opt(whole, nanos as u32)
        .single()
        .map(|timestamp| timestamp.to_rfc3339())
}
