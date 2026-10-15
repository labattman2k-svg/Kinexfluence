//! Fixture-driven tests for the ChatGPT export parser.
//!
//! The fixture at `fixtures/chatgpt/sample-export.json` is SYNTHETIC. It was
//! hand-written to match the documented ChatGPT export shape; it is not a
//! real capture, and it is labelled as such inside the file itself.
//!
//! That is a real limitation: these tests hold the parser to our assumptions
//! about the format rather than to the format itself. Validating against a
//! real sanitized export is tracked as issue #6 and must happen before
//! PR #3b, which is where a wrong assumption reaches the database.

use kfx_adapters::chatgpt::{ChatGptAdapter, EXPORT_SHAPE};
use kfx_adapters::provider::{AdapterError, ProviderAdapter, RawCapture};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../fixtures/chatgpt/sample-export.json");
const GOLDEN: &str = include_str!("../../../fixtures/chatgpt/sample-export.expected.json");

fn parse_fixture() -> RawCapture {
    ChatGptAdapter
        .parse_export(FIXTURE.as_bytes())
        .expect("the synthetic fixture should parse")
}

fn text_of(message: &Value) -> Option<&str> {
    message
        .get("blocks")?
        .as_array()?
        .iter()
        .find_map(|block| block.get("text").and_then(Value::as_str))
}

fn all_text(capture: &RawCapture) -> String {
    capture
        .messages
        .iter()
        .filter_map(text_of)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn parses_minimal_conversation() {
    let capture = parse_fixture();

    assert_eq!(capture.source, "chatgpt");
    assert_eq!(
        capture.messages.len(),
        5,
        "the active path holds five real messages plus one null-message placeholder"
    );

    let roles: Vec<&str> = capture
        .messages
        .iter()
        .map(|message| {
            message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        roles,
        vec!["system", "user", "assistant", "assistant", "assistant"]
    );

    for (index, message) in capture.messages.iter().enumerate() {
        assert_eq!(
            message.get("turn_index").and_then(Value::as_u64),
            Some(index as u64),
            "turn_index should count emitted messages from 0"
        );
    }

    assert_eq!(
        text_of(&capture.messages[0]),
        Some("You are a helpful assistant.")
    );

    // Trait-contract assertions for the new methods live in this test rather
    // than in a tenth one, so the suite stays at the brief's pinned count of
    // 24 (section 8). STANDARDS.md section 1 requires every public function
    // to have a test, and `matches_url`/`supported_export_versions` are public.
    assert_eq!(ChatGptAdapter.id(), "chatgpt");
    assert_eq!(ChatGptAdapter.supported_export_versions(), &[EXPORT_SHAPE]);
    assert!(ChatGptAdapter.matches_url("https://chatgpt.com/c/abc"));
    assert!(ChatGptAdapter.matches_url("https://chat.openai.com/c/abc"));
    assert!(!ChatGptAdapter.matches_url("https://claude.ai/chat/abc"));
}

#[test]
fn parses_code_block_as_content_block() {
    let capture = parse_fixture();

    let code = capture
        .messages
        .iter()
        .filter_map(|message| message.get("blocks").and_then(Value::as_array))
        .flatten()
        .find(|block| block.get("type").and_then(Value::as_str) == Some("code"))
        .expect("a code block should be present");

    assert_eq!(code.get("language").and_then(Value::as_str), Some("python"));
    assert_eq!(
        code.get("text").and_then(Value::as_str),
        Some("def add(a, b):\n    return a + b"),
        "the markdown fence should be stripped"
    );
}

#[test]
fn walks_branch_to_current_node() {
    let capture = parse_fixture();
    let text = all_text(&capture);

    assert_eq!(
        capture.messages.len(),
        5,
        "only the active path should be captured"
    );
    assert!(
        text.contains("Here is a shorter version."),
        "the active path must be included"
    );
    assert!(
        !text.contains("Actually, add type hints."),
        "the sibling off current_node must be excluded"
    );
}

#[test]
fn unknown_content_type_preserved_in_metadata() {
    let capture = parse_fixture();

    let last = capture.messages.last().expect("at least one message");
    let unknown = last
        .get("metadata_json")
        .and_then(|metadata| metadata.get("unknown_blocks"))
        .and_then(Value::as_array)
        .expect("the unknown block should be preserved");

    assert_eq!(unknown.len(), 1);
    assert_eq!(
        unknown[0].get("content_type").and_then(Value::as_str),
        Some("tether_browsing_display"),
        "the raw block should be preserved verbatim"
    );
    assert!(
        last.get("blocks")
            .and_then(Value::as_array)
            .expect("blocks is always present")
            .is_empty(),
        "an unknown content type should produce no canonical blocks"
    );
}

#[test]
fn null_create_time_handled() {
    let capture = parse_fixture();

    let message = capture
        .messages
        .iter()
        .find(|message| message.get("turn_index").and_then(Value::as_u64) == Some(3))
        .expect("turn 3 should exist");

    assert_eq!(
        message.get("create_time"),
        Some(&Value::Null),
        "a null timestamp must stay null, not be coerced to 0"
    );
}

#[test]
fn malformed_json_returns_invalid_format() {
    let error = ChatGptAdapter
        .parse_export(b"{ this is not json")
        .expect_err("bad input should not parse");

    assert!(matches!(error, AdapterError::InvalidFormat));
}

#[test]
fn missing_mapping_returns_malformed_message() {
    let error = ChatGptAdapter
        .parse_export(br#"[{"id":"c1","title":"no mapping here"}]"#)
        .expect_err("a conversation without a mapping should fail");

    match error {
        AdapterError::MalformedMessage { index, reason } => {
            assert_eq!(index, 0);
            assert!(
                reason.contains("mapping"),
                "the reason should name the missing field, got: {reason}"
            );
        }
        other => panic!("expected MalformedMessage, got {other:?}"),
    }
}

#[test]
fn idempotent_parse() {
    let first = serde_json::to_value(parse_fixture()).expect("capture serializes");
    let second = serde_json::to_value(parse_fixture()).expect("capture serializes");

    assert_eq!(first, second, "parsing the same bytes twice must be stable");
}

#[test]
fn golden_fixture_matches() {
    let actual = serde_json::to_value(parse_fixture()).expect("capture serializes");
    let expected: Value =
        serde_json::from_str(GOLDEN).expect("the golden file should be valid JSON");

    assert_eq!(
        actual, expected,
        "parser output drifted from the golden file. If the change is intended, \
         regenerate the golden file and hand-verify it against section 4 of the \
         PR #3a brief."
    );
}
