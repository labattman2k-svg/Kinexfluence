//! Snippet extraction tests.
//!
//! Inputs are built inline with `serde_json::json!` rather than loaded from
//! fixture files: these tests are about the extraction rules, and a fixture
//! would obscure which field is being exercised. Adapter fixture testing is
//! PR #3a's job and lives in `crates/adapters/tests`.
//!
//! These drive `extract_snippets` directly. Whether a whole capture turns into
//! the right rows is a question about `normalize`, and that is PR #3c-2. Going
//! through the orchestrator here would have meant every assertion about a
//! paragraph break was also an assertion about id generation and parent
//! resolution, which makes a failure harder to read than it needs to be.

use kfx_core::normalizer::snippets::{KIND_CODE, KIND_TEXT};
use kfx_core::normalizer::{content_hash, extract_snippets, CanonicalMessage, NewSnippet};
use serde_json::{json, Value};

/// Deserialize a canonical message from just its block array.
///
/// Built by deserializing rather than by naming every field, so these tests
/// break if `CanonicalMessage` stops accepting what the parser emits.
fn message(blocks: Value) -> CanonicalMessage {
    serde_json::from_value(json!({
        "external_id": "n1",
        "parent_external_id": null,
        "role": "assistant",
        "turn_index": 0,
        "create_time": null,
        "blocks": blocks,
        "raw": {},
    }))
    .expect("a canonical message should deserialize")
}

fn snippets_of(blocks: Value) -> Vec<NewSnippet> {
    extract_snippets(&message(blocks), "m1")
}

#[test]
fn text_block_splits_into_paragraphs() {
    let snippets = snippets_of(json!([{
        "type": "text",
        "text": "First paragraph.\n\nSecond paragraph.\n\nThird paragraph."
    }]));

    assert_eq!(snippets.len(), 3);
    assert_eq!(snippets[0].text, "First paragraph.");
    assert_eq!(snippets[1].text, "Second paragraph.");
    assert_eq!(snippets[2].text, "Third paragraph.");

    for snippet in &snippets {
        assert_eq!(snippet.granularity, "paragraph");
        assert_eq!(snippet.kind, KIND_TEXT);
        assert_eq!(snippet.block_index, 0);
        assert_eq!(snippet.language, None);
    }

    assert_eq!(snippets[0].char_start, Some(0));
    assert_eq!(snippets[0].char_end, Some(16));
    assert_eq!(
        snippets[1].char_start,
        Some(18),
        "the second paragraph starts after the blank line"
    );
}

#[test]
fn code_block_stays_whole() {
    let snippets = snippets_of(json!([{
        "type": "code",
        "language": "python",
        "text": "def add(a, b):\n\n    return a + b\n"
    }]));

    assert_eq!(
        snippets.len(),
        1,
        "a blank line inside code must not split it: half a function is not a usable fragment"
    );
    assert_eq!(snippets[0].granularity, "block");
    assert_eq!(snippets[0].kind, KIND_CODE);
    assert_eq!(snippets[0].language.as_deref(), Some("python"));
}

#[test]
fn execution_output_becomes_a_text_snippet() {
    let snippets = snippets_of(json!([{ "type": "execution_output", "text": "42" }]));

    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0].kind, KIND_TEXT);
    assert_eq!(snippets[0].granularity, "block");
    assert_eq!(snippets[0].text, "42");
}

#[test]
fn unknown_block_type_produces_no_snippet_but_does_not_stop_the_message() {
    let snippets = snippets_of(json!([
        { "type": "text", "text": "usable prose" },
        { "type": "tether_browsing_display", "parts": [{ "url": "https://example.com" }] },
        { "type": "text", "text": "more prose" }
    ]));

    assert_eq!(
        snippets.len(),
        2,
        "the unknown block should be skipped, not the message"
    );
    assert_eq!(snippets[0].text, "usable prose");
    assert_eq!(snippets[1].text, "more prose");
    assert_eq!(
        snippets[1].block_index, 2,
        "block_index should keep pointing at the real source block"
    );
}

#[test]
fn snippet_hash_ignores_case_and_interior_whitespace() {
    let snippets = snippets_of(json!([
        { "type": "text", "text": "Hello   World" },
        { "type": "text", "text": "  hello world  " },
        { "type": "text", "text": "hello\tworld" }
    ]));

    assert_eq!(snippets.len(), 3);
    let hashes: Vec<&str> = snippets.iter().map(|s| s.hash.as_str()).collect();

    assert_eq!(
        hashes[0], hashes[1],
        "case and leading/trailing space should not change the hash"
    );
    assert_eq!(
        hashes[0], hashes[2],
        "a tab and a space run should normalize the same"
    );

    // The stored text keeps its original form; only the hash is normalized.
    assert_eq!(snippets[0].text, "Hello   World");
    assert_eq!(hashes[0], content_hash("hello world"));
}

#[test]
fn segment_index_increments_across_blocks_within_a_message() {
    let snippets = snippets_of(json!([
        { "type": "text", "text": "para one\n\npara two" },
        { "type": "code", "language": null, "text": "print(1)" }
    ]));

    assert_eq!(snippets.len(), 3);
    let segments: Vec<i64> = snippets.iter().map(|s| s.segment_index).collect();
    let blocks: Vec<i64> = snippets.iter().map(|s| s.block_index).collect();

    assert_eq!(
        segments,
        vec![0, 1, 2],
        "segment_index is one counter across the message, per the brief"
    );
    assert_eq!(blocks, vec![0, 0, 1]);

    assert!(
        snippets.iter().all(|s| s.message_id == "m1"),
        "every snippet should point at the message it was extracted from"
    );
}
