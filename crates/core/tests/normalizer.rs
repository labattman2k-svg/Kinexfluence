//! Normalizer tests.
//!
//! Inputs are built inline with `serde_json::json!` rather than loaded from
//! fixture files: these tests are about the transformation rules, and a
//! fixture would obscure which field is being exercised. Adapter fixture
//! testing is PR #3a's job and lives in `crates/adapters/tests`.
//!
//! Snippet extraction is tested against `extract_snippets` directly in PR #3c,
//! where it landed. What is tested here is the orchestration around it -- id
//! assignment, parent resolution, timestamps, attachments -- plus one
//! end-to-end check that a capture's snippets survive the trip through
//! `normalize`. Keeping the two apart means a failure points at one layer
//! instead of at whichever of the two the test happened to route through.

use kfx_adapters::RawCapture;
use kfx_core::normalizer::snippets::{KIND_CODE, KIND_TEXT};
use kfx_core::normalizer::{
    content_hash, extract_snippets, normalize, CanonicalMessage, NewSnippet, NormalizedBatch,
};
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

fn capture(conversation: Value, messages: Vec<Value>, attachments: Vec<Value>) -> RawCapture {
    RawCapture {
        source: "chatgpt".to_owned(),
        conversation,
        messages,
        attachments,
    }
}

fn conversation(id: &str) -> Value {
    json!({
        "external_id": id,
        "title": "A conversation",
        "create_time": 1_700_000_000.0,
        "update_time": 1_700_000_060.0,
        "model": "gpt-4",
        "url": null,
    })
}

fn text_message(external_id: &str, parent: Option<&str>, text: &str) -> Value {
    json!({
        "external_id": external_id,
        "parent_external_id": parent,
        "role": "assistant",
        "turn_index": 0,
        "create_time": 1_700_000_000.0,
        "blocks": [{ "type": "text", "text": text }],
        "raw": { "id": external_id, "message": { "content": { "parts": [text] } } },
    })
}

fn batch_for(captures: &[RawCapture]) -> NormalizedBatch {
    normalize(captures).expect("normalization should succeed")
}

// ---------------------------------------------------------------------------
// Conversations
// ---------------------------------------------------------------------------

#[test]
fn normalize_produces_one_conversation_per_capture() {
    let captures = vec![
        capture(
            conversation("c1"),
            vec![text_message("n1", None, "one")],
            vec![],
        ),
        capture(
            conversation("c2"),
            vec![text_message("n1", None, "two")],
            vec![],
        ),
    ];

    let batch = batch_for(&captures);

    assert_eq!(batch.conversations.len(), 2);
    assert_eq!(
        batch.conversations[0].external_id.as_deref(),
        Some("c1"),
        "order should follow the input"
    );
    assert_eq!(batch.conversations[1].external_id.as_deref(), Some("c2"));
    assert_eq!(batch.conversations[0].source_id, "chatgpt");
    assert_eq!(batch.messages.len(), 2, "one message per capture");
}

#[test]
fn conversation_timestamps_convert_from_epoch_to_rfc3339() {
    let captures = vec![capture(
        conversation("c1"),
        vec![text_message("n1", None, "one")],
        vec![],
    )];

    let batch = batch_for(&captures);

    assert_eq!(
        batch.conversations[0].created_at.as_deref(),
        Some("2023-11-14T22:13:20+00:00"),
        "epoch seconds should become RFC 3339 UTC"
    );
    assert_eq!(
        batch.conversations[0].updated_at.as_deref(),
        Some("2023-11-14T22:14:20+00:00")
    );
}

#[test]
fn null_timestamp_becomes_none() {
    let mut convo = conversation("c1");
    convo["create_time"] = Value::Null;

    let captures = vec![capture(
        convo,
        vec![text_message("n1", None, "one")],
        vec![],
    )];

    let batch = batch_for(&captures);

    assert_eq!(
        batch.conversations[0].created_at, None,
        "a null timestamp must stay absent, not become the epoch"
    );
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[test]
fn message_parent_ids_resolve_to_generated_message_ids() {
    let messages = vec![
        text_message("n1", None, "first"),
        text_message("n2", Some("n1"), "second"),
        text_message("n3", Some("n2"), "third"),
    ];
    let captures = vec![capture(conversation("c1"), messages, vec![])];

    let batch = batch_for(&captures);

    assert_eq!(batch.messages.len(), 3);
    assert_eq!(batch.messages[0].parent_id, None, "the root has no parent");

    assert_eq!(
        batch.messages[1].parent_id.as_deref(),
        Some(batch.messages[0].id.as_str()),
        "parent_external_id should resolve to the parent's generated id"
    );
    assert_eq!(
        batch.messages[2].parent_id.as_deref(),
        Some(batch.messages[1].id.as_str())
    );

    assert_eq!(
        batch.messages[1].conversation_id, batch.conversations[0].id,
        "every message belongs to the conversation that produced it"
    );
}

#[test]
fn message_raw_json_keeps_the_source_node_verbatim() {
    let message = text_message("n1", None, "hello");
    let captures = vec![capture(conversation("c1"), vec![message.clone()], vec![])];

    let batch = batch_for(&captures);

    let raw = batch.messages[0]
        .raw_json
        .as_deref()
        .expect("raw_json should be populated");

    assert_eq!(
        serde_json::from_str::<Value>(raw).expect("raw_json should be valid JSON"),
        message["raw"],
        "raw_json should be the source node unchanged"
    );

    assert_eq!(
        serde_json::from_str::<Value>(&batch.messages[0].content_json)
            .expect("content_json should be valid JSON"),
        message["blocks"],
        "content_json should be the canonical block array"
    );
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------

#[test]
fn attachment_status_follows_hash_and_blob_path() {
    let captures = vec![capture(
        conversation("c1"),
        vec![text_message("n1", None, "one")],
        vec![
            json!({
                "external_id": "file-missing",
                "filename": "notes.txt",
                "mime": "text/plain",
                "status": "missing"
            }),
            json!({
                "external_id": "file-present",
                "filename": "kept.txt",
                "mime": "text/plain",
                "hash": "abc123",
                "blob_path": "/var/blobs/abc123"
            }),
        ],
    )];

    let batch = batch_for(&captures);

    assert_eq!(batch.attachments.len(), 2);

    let missing = &batch.attachments[0];
    assert_eq!(
        missing.status, "missing",
        "a reference with no hash and no blob_path is what migration 0002 exists for"
    );
    assert_eq!(missing.hash, None);
    assert_eq!(missing.blob_path, None);
    assert_eq!(missing.owner_type, "conversation");
    assert_eq!(missing.owner_id, batch.conversations[0].id);

    let present = &batch.attachments[1];
    assert_eq!(present.status, "present");
    assert_eq!(present.hash.as_deref(), Some("abc123"));
    assert_eq!(present.blob_path.as_deref(), Some("/var/blobs/abc123"));
}

// ---------------------------------------------------------------------------
// End to end
// ---------------------------------------------------------------------------

#[test]
fn normalize_extracts_snippets_from_a_capture() {
    let captures = vec![capture(
        conversation("c1"),
        vec![text_message("n1", None, "First.\n\nSecond.")],
        vec![],
    )];

    let batch = batch_for(&captures);

    assert_eq!(
        batch.snippets.len(),
        2,
        "normalize should delegate to the extractor rather than return an empty vec"
    );
    assert_eq!(batch.snippets[0].text, "First.");
    assert_eq!(batch.snippets[1].text, "Second.");
    assert!(
        batch
            .snippets
            .iter()
            .all(|s| s.message_id == batch.messages[0].id),
        "snippets should reference the message normalize just created, not a provider id"
    );
}

// ---------------------------------------------------------------------------
// Snippet extraction
//
// These drive `extract_snippets` directly, so an assertion about a paragraph
// break is not also an assertion about id generation. Whether a whole capture
// turns into the right rows is covered above.
// ---------------------------------------------------------------------------

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

    // These are byte offsets, but every character in this sample is one byte,
    // so they would read the same either way. `paragraph_offsets_are_bytes`
    // below is the test that actually tells the two apart.
    assert_eq!(snippets[0].char_start, Some(0));
    assert_eq!(snippets[0].char_end, Some(16));
    assert_eq!(
        snippets[1].char_start,
        Some(18),
        "the second paragraph starts after the blank line"
    );
}

#[test]
fn paragraph_offsets_are_bytes_not_characters() {
    // "café" is four characters but five bytes. Character offsets would end
    // the first paragraph at 4 and start the second at 6.
    let snippets = snippets_of(json!([{
        "type": "text",
        "text": "café\n\nsecond"
    }]));

    assert_eq!(snippets.len(), 2);
    assert_eq!(snippets[0].text, "café");
    assert_eq!(snippets[0].char_start, Some(0));
    assert_eq!(
        snippets[0].char_end,
        Some(5),
        "café is five bytes but only four characters"
    );
    assert_eq!(
        snippets[1].char_start,
        Some(7),
        "two newlines follow the five bytes of the first paragraph"
    );
    assert_eq!(snippets[1].char_end, Some(13));
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
    assert_eq!(
        snippets[0].char_start, None,
        "the block is the unit, so there is no sub-block span to give"
    );
    assert_eq!(snippets[0].char_end, None);
}

#[test]
fn execution_output_becomes_a_text_snippet() {
    let snippets = snippets_of(json!([{ "type": "execution_output", "text": "42" }]));

    assert_eq!(snippets.len(), 1);
    assert_eq!(snippets[0].kind, KIND_TEXT);
    assert_eq!(snippets[0].granularity, "block");
    assert_eq!(snippets[0].text, "42");
    assert_eq!(
        snippets[0].char_start, None,
        "block granularity means the block is the unit"
    );
    assert_eq!(snippets[0].char_end, None);
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
