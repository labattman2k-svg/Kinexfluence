//! The ChatGPT export adapter.
//!
//! Parses the provider's `conversations.json` into the canonical capture
//! shape. Pure library work: no database access, no network access.
//!
//! Known limitation, recorded in the ADR 0007 addendum: only the active path
//! of a branched conversation is captured. Siblings off `current_node` are
//! ignored.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

use super::types::{
    CanonicalCapture, CaptureAttachment, CaptureConversation, CaptureMessage, CaptureMetadata,
    ContentBlock, RawAttachment, RawContent, RawConversation, RawMessage, RawNode,
};
use crate::provider::{AdapterError, ProviderAdapter, RawCapture};

/// Identifier for the export shape this adapter accepts.
///
/// ChatGPT does not put a version number in its export, so this names the
/// shape rather than inventing a provider-declared version.
pub const EXPORT_SHAPE: &str = "chatgpt-conversations-json";

/// Lowercased host of a URL, without userinfo or port.
///
/// Deliberately a small hand-rolled parse rather than a dependency: the input
/// is a URL the user is currently on, not untrusted markup, and `None` simply
/// means "not ours".
fn host_of(url: &str) -> Option<String> {
    let after_scheme = url
        .trim()
        .split_once("://")
        .map_or(url.trim(), |(_, rest)| rest);
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?; // drop any userinfo
    let host = host.split(':').next()?; // drop any port

    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// Parses ChatGPT's `conversations.json` export file.
pub struct ChatGptAdapter;

impl ProviderAdapter for ChatGptAdapter {
    fn id(&self) -> &'static str {
        "chatgpt"
    }

    fn matches_url(&self, url: &str) -> bool {
        // Matched against the host, not the whole URL. A substring check would
        // let `https://example.com/chatgpt.com` or a `?ref=chatgpt.com` query
        // parameter claim to be a ChatGPT capture.
        let Some(host) = host_of(url) else {
            return false;
        };

        host == "chatgpt.com"
            || host.ends_with(".chatgpt.com")
            || host == "chat.openai.com"
            || host.ends_with(".chat.openai.com")
    }

    fn supported_export_versions(&self) -> &[&'static str] {
        &[EXPORT_SHAPE]
    }

    fn parse_export(&self, bytes: &[u8]) -> Result<Vec<RawCapture>, AdapterError> {
        let conversations: Vec<RawConversation> =
            serde_json::from_slice(bytes).map_err(|_| AdapterError::InvalidFormat)?;

        conversations
            .into_iter()
            .enumerate()
            .map(|(index, conversation)| parse_conversation(index, conversation)?.into_raw())
            .collect()
    }
}

fn parse_conversation(
    index: usize,
    raw: RawConversation,
) -> Result<CanonicalCapture, AdapterError> {
    let mapping = raw.mapping.ok_or_else(|| AdapterError::MalformedMessage {
        index,
        reason: "conversation has no mapping".to_string(),
    })?;

    let external_id =
        raw.id
            .or(raw.conversation_id)
            .ok_or_else(|| AdapterError::MalformedMessage {
                index,
                reason: "conversation has neither id nor conversation_id".to_string(),
            })?;

    if raw.current_node.is_none() {
        warn!(
            target: "kfx.adapters.chatgpt",
            "conversation has no current_node; no messages reconstructed"
        );
    }

    let mut messages = Vec::new();
    let mut attachments = Vec::new();

    for (node_id, raw_node, node) in active_path(&mapping, raw.current_node.as_deref()) {
        let Some(message) = node.message.as_ref() else {
            // Structural placeholder, not an error.
            continue;
        };

        let turn_index = messages.len();
        let (blocks, metadata_json) = blocks_for(message);
        attachments.extend(attachments_for(message));

        messages.push(CaptureMessage {
            external_id: node_id,
            parent_external_id: node.parent.clone(),
            role: message
                .author
                .as_ref()
                .and_then(|author| author.role.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            turn_index,
            create_time: message.create_time.clone(),
            blocks,
            metadata_json,
            // The mapping node verbatim, so PR #3b can fill `message.raw_json`
            // without re-parsing the export.
            raw: raw_node.clone(),
        });
    }

    Ok(CanonicalCapture {
        source: "chatgpt".to_string(),
        conversation: CaptureConversation {
            external_id,
            title: raw.title,
            create_time: raw.create_time,
            update_time: raw.update_time,
            model: raw.default_model_slug.or(raw.model),
            url: None,
        },
        messages,
        attachments,
    })
}

/// Walk from `current_node` back through `parent` to the root, reversed into
/// chronological order.
///
/// The walk is bounded by the mapping size so a malformed export containing a
/// `parent` cycle terminates instead of looping forever.
fn active_path<'a>(
    mapping: &'a HashMap<String, Value>,
    current_node: Option<&str>,
) -> Vec<(String, &'a Value, RawNode)> {
    let limit = mapping.len() + 1;
    let mut path: Vec<(String, &Value, RawNode)> = Vec::new();
    let mut cursor = current_node.map(str::to_owned);

    while let Some(id) = cursor {
        if path.len() >= limit {
            warn!(
                target: "kfx.adapters.chatgpt",
                "parent chain exceeded mapping size; treating as a cycle"
            );
            break;
        }
        let Some(raw_node) = mapping.get(&id) else {
            break;
        };
        // Deserialized from a borrow so the raw node can be kept verbatim
        // without being cloned until the message is built.
        let Ok(node) = RawNode::deserialize(raw_node) else {
            warn!(
                target: "kfx.adapters.chatgpt",
                "mapping node did not match the expected shape; chain ends here"
            );
            break;
        };
        cursor = node.parent.clone();
        path.push((id, raw_node, node));
    }

    path.reverse();
    path
}

/// Map a message's `content` into canonical blocks.
///
/// Returns the blocks plus, when anything was unrecognised, the raw pieces
/// preserved verbatim. An unrecognised content type never fails the parse.
fn blocks_for(message: &RawMessage) -> (Vec<ContentBlock>, Option<CaptureMetadata>) {
    if message.content.is_null() {
        return (Vec::new(), None);
    }

    let parsed: RawContent = match RawContent::deserialize(&message.content) {
        Ok(content) => content,
        Err(_) => {
            warn!(
                target: "kfx.adapters.chatgpt",
                "message content did not match the expected shape; preserved raw"
            );
            return (
                Vec::new(),
                Some(CaptureMetadata {
                    unknown_blocks: vec![message.content.clone()],
                }),
            );
        }
    };

    let mut unknown: Vec<Value> = Vec::new();
    let content_type = parsed.content_type.as_deref().unwrap_or_default();

    let blocks = match content_type {
        "text" | "multimodal_text" => text_blocks(&parsed.parts, &mut unknown),
        "code" => code_blocks(&parsed.parts, &mut unknown),
        "execution_output" => execution_blocks(&parsed.parts, &mut unknown),
        other => {
            warn!(
                target: "kfx.adapters.chatgpt",
                content_type = other,
                "unrecognised content type; preserved raw and skipped"
            );
            // The whole block, verbatim, with nothing dropped.
            unknown.push(message.content.clone());
            Vec::new()
        }
    };

    let metadata_json = if unknown.is_empty() {
        None
    } else {
        Some(CaptureMetadata {
            unknown_blocks: unknown,
        })
    };

    (blocks, metadata_json)
}

fn text_blocks(parts: &[Value], unknown: &mut Vec<Value>) -> Vec<ContentBlock> {
    let mut pieces: Vec<String> = Vec::new();
    for part in parts {
        match part {
            Value::String(text) => pieces.push(text.clone()),
            other => unknown.push(other.clone()),
        }
    }

    if pieces.is_empty() {
        Vec::new()
    } else {
        vec![ContentBlock::Text {
            text: pieces.join("\n"),
        }]
    }
}

fn execution_blocks(parts: &[Value], unknown: &mut Vec<Value>) -> Vec<ContentBlock> {
    let mut pieces: Vec<String> = Vec::new();
    for part in parts {
        match part {
            Value::String(text) => pieces.push(text.clone()),
            Value::Object(map) => match map.get("text").and_then(Value::as_str) {
                Some(text) => pieces.push(text.to_string()),
                None => unknown.push(part.clone()),
            },
            other => unknown.push(other.clone()),
        }
    }

    if pieces.is_empty() {
        Vec::new()
    } else {
        vec![ContentBlock::ExecutionOutput {
            text: pieces.join("\n"),
        }]
    }
}

/// ChatGPT can express code either as a fenced markdown string or as a
/// structured part carrying `language` and `text`. Both are handled.
fn code_blocks(parts: &[Value], unknown: &mut Vec<Value>) -> Vec<ContentBlock> {
    let mut pieces: Vec<String> = Vec::new();
    let mut language: Option<String> = None;

    for part in parts {
        match part {
            Value::String(raw) => {
                let (fence_language, body) = split_fence(raw);
                if language.is_none() {
                    language = fence_language;
                }
                pieces.push(body.to_string());
            }
            Value::Object(map) => {
                if language.is_none() {
                    language = map
                        .get("language")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned);
                }
                match map.get("text").and_then(Value::as_str) {
                    Some(text) => pieces.push(text.to_string()),
                    None => unknown.push(part.clone()),
                }
            }
            other => unknown.push(other.clone()),
        }
    }

    if pieces.is_empty() {
        Vec::new()
    } else {
        vec![ContentBlock::Code {
            language,
            text: pieces.join("\n"),
        }]
    }
}

/// Split a fenced markdown block into its language tag and body.
///
/// A string that is not fenced is returned unchanged with no language.
fn split_fence(raw: &str) -> (Option<String>, &str) {
    let trimmed = raw.trim_start_matches('\n');
    let Some(rest) = trimmed.strip_prefix("```") else {
        return (None, raw);
    };

    let Some(newline) = rest.find('\n') else {
        return (None, raw);
    };

    let tag = rest[..newline].trim();
    let body = rest[newline + 1..]
        .strip_suffix("```")
        .unwrap_or(&rest[newline + 1..]);
    let body = body.strip_suffix('\n').unwrap_or(body);

    let language = if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    };

    (language, body)
}

/// Attachment references from a message's metadata.
///
/// References only; nothing is fetched (ADR 0007). Entries without an id
/// cannot be recorded honestly, so they are skipped with a warning.
fn attachments_for(message: &RawMessage) -> Vec<CaptureAttachment> {
    let Some(list) = message
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("attachments"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in list {
        match serde_json::from_value::<RawAttachment>(item.clone()) {
            Ok(attachment) => match attachment.id {
                Some(external_id) => out.push(CaptureAttachment {
                    external_id,
                    filename: attachment.filename.or(attachment.name),
                    mime: attachment.mime_type.or(attachment.mime),
                    status: "missing".to_string(),
                }),
                None => warn!(
                    target: "kfx.adapters.chatgpt",
                    "attachment reference has no id; skipped"
                ),
            },
            Err(_) => warn!(
                target: "kfx.adapters.chatgpt",
                "attachment reference did not match the expected shape; skipped"
            ),
        }
    }

    out
}
