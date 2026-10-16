//! Writer tests.
//!
//! Every test runs against a real migrated database through `#[sqlx::test]`
//! rather than a mock. Most of what `write_batch` promises is about what
//! SQLite does with the rows -- constraints, foreign keys, transaction
//! rollback -- and a mock would only assert that the writer called the mock.
//!
//! The batches are hand-built instead of produced by `normalize`, so a test
//! can express a batch the normalizer would never emit. That is exactly what
//! the constraint tests need.

use kfx_core::normalizer::{
    NewAttachment, NewConversation, NewMessage, NewSnippet, NormalizedBatch,
};
use kfx_core::writer::{write_batch, ImportSummary, WriteError};
use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

fn conversation(id: &str, content_hash: Option<&str>) -> NewConversation {
    NewConversation {
        id: id.to_owned(),
        source_id: "chatgpt".to_owned(),
        external_id: Some(format!("ext-{id}")),
        title: Some(format!("Title {id}")),
        url: None,
        model: Some("gpt-4".to_owned()),
        created_at: Some("2023-11-14T22:13:20+00:00".to_owned()),
        updated_at: Some("2023-11-14T22:14:20+00:00".to_owned()),
        raw_path: None,
        metadata_json: None,
        content_hash: content_hash.map(str::to_owned),
    }
}

fn message(id: &str, conversation_id: &str, turn_index: i64) -> NewMessage {
    NewMessage {
        id: id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        parent_id: None,
        role: "assistant".to_owned(),
        turn_index,
        created_at: Some("2023-11-14T22:13:20+00:00".to_owned()),
        content_json: "[]".to_owned(),
        raw_json: None,
    }
}

fn snippet(id: &str, message_id: &str, text: &str) -> NewSnippet {
    NewSnippet {
        id: id.to_owned(),
        message_id: message_id.to_owned(),
        block_index: 0,
        segment_index: 0,
        granularity: "block".to_owned(),
        kind: "text".to_owned(),
        text: text.to_owned(),
        language: None,
        char_start: None,
        char_end: None,
        hash: format!("hash-{id}"),
    }
}

fn attachment(
    id: &str,
    owner_id: &str,
    status: &str,
    hash: Option<&str>,
    blob_path: Option<&str>,
) -> NewAttachment {
    NewAttachment {
        id: id.to_owned(),
        owner_type: "conversation".to_owned(),
        owner_id: owner_id.to_owned(),
        filename: Some(format!("{id}.txt")),
        mime: Some("text/plain".to_owned()),
        status: status.to_owned(),
        hash: hash.map(str::to_owned),
        blob_path: blob_path.map(str::to_owned),
    }
}

fn batch(
    conversations: Vec<NewConversation>,
    messages: Vec<NewMessage>,
    snippets: Vec<NewSnippet>,
    attachments: Vec<NewAttachment>,
) -> NormalizedBatch {
    NormalizedBatch {
        conversations,
        messages,
        snippets,
        attachments,
    }
}

/// Count the rows in a table.
///
/// The table name comes from a literal in this file, never from input; a bound
/// parameter cannot stand in for an identifier.
async fn count(pool: &SqlitePool, table: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_inserts_all_rows(pool: SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let input = batch(
        vec![conversation("c1", Some("hash-c1"))],
        vec![message("m1", "c1", 0), message("m2", "c1", 1)],
        vec![snippet("s1", "m1", "one"), snippet("s2", "m2", "two")],
        vec![attachment("a1", "c1", "missing", None, None)],
    );

    let summary = write_batch(&pool, input).await?;

    assert_eq!(
        summary,
        ImportSummary {
            conversations_inserted: 1,
            conversations_skipped: 0,
            messages_inserted: 2,
            snippets_inserted: 2,
            attachments_inserted: 1,
            attachments_missing: 1,
        }
    );

    assert_eq!(count(&pool, "conversation").await?, 1);
    assert_eq!(count(&pool, "message").await?, 2);
    assert_eq!(count(&pool, "snippet").await?, 2);
    assert_eq!(count(&pool, "attachment").await?, 1);

    // `conversation.source_id` references `source(id)` and no migration seeds
    // that table, so the writer has to create it or every conversation insert
    // fails the foreign key.
    let source: String = sqlx::query_scalar("SELECT id FROM source")
        .fetch_one(&pool)
        .await?;
    assert_eq!(source, "chatgpt");

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_handles_empty_batch(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let summary = write_batch(&pool, NormalizedBatch::default()).await?;

    assert_eq!(summary, ImportSummary::default());
    assert_eq!(count(&pool, "conversation").await?, 0);
    assert_eq!(count(&pool, "message").await?, 0);

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_preserves_message_order(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = batch(
        vec![conversation("c1", Some("hash-c1"))],
        vec![
            message("m2", "c1", 1),
            message("m1", "c1", 0),
            message("m3", "c1", 2),
        ],
        vec![],
        vec![],
    );

    write_batch(&pool, input).await?;

    let ordered: Vec<String> =
        sqlx::query_scalar("SELECT id FROM message WHERE conversation_id = ? ORDER BY turn_index")
            .bind("c1")
            .fetch_all(&pool)
            .await?;

    assert_eq!(
        ordered,
        vec!["m1", "m2", "m3"],
        "a reader sorts by turn_index, so the insert order must not decide the reading order"
    );

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_links_snippets_to_messages(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = batch(
        vec![conversation("c1", Some("hash-c1"))],
        vec![message("m1", "c1", 0)],
        vec![snippet("s1", "m1", "one"), snippet("s2", "m1", "two")],
        vec![],
    );

    write_batch(&pool, input).await?;

    let linked: Vec<String> = sqlx::query_scalar("SELECT message_id FROM snippet ORDER BY id")
        .fetch_all(&pool)
        .await?;

    assert_eq!(
        linked,
        vec!["m1", "m1"],
        "both snippets should point at the message row, not at a provider id"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_records_missing_attachments(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = batch(
        vec![conversation("c1", Some("hash-c1"))],
        vec![message("m1", "c1", 0)],
        vec![],
        vec![
            // What a ChatGPT export actually produces: a reference with no
            // file behind it.
            attachment("a1", "c1", "missing", None, None),
            attachment("a2", "c1", "present", Some("abc123"), Some("/blobs/abc123")),
        ],
    );

    let summary = write_batch(&pool, input).await?;

    assert_eq!(
        summary.attachments_inserted, 2,
        "a missing file is still a row: the reference is worth keeping"
    );
    assert_eq!(summary.attachments_missing, 1);
    assert_eq!(
        summary.attachments_present(),
        1,
        "attachments_inserted counts every row, so present is the difference"
    );

    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM attachment ORDER BY id")
        .fetch_all(&pool)
        .await?;
    assert_eq!(statuses, vec!["missing", "present"]);

    Ok(())
}

// ---------------------------------------------------------------------------
// Re-import
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_skips_duplicate_conversation(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = batch(
        vec![conversation("c1", Some("hash-c1"))],
        vec![message("m1", "c1", 0)],
        vec![snippet("s1", "m1", "one")],
        vec![attachment("a1", "c1", "missing", None, None)],
    );
    write_batch(&pool, first).await?;

    // A second normalization of the same file mints fresh row ids, so nothing
    // but the conversation's content hash can connect the two runs. That is
    // the case the hash exists for.
    let second = batch(
        vec![conversation("c9", Some("hash-c1"))],
        vec![message("m9", "c9", 0)],
        vec![snippet("s9", "m9", "one")],
        vec![attachment("a9", "c9", "missing", None, None)],
    );
    let summary = write_batch(&pool, second).await?;

    assert_eq!(summary.conversations_inserted, 0);
    assert_eq!(summary.conversations_skipped, 1);
    assert_eq!(
        summary.messages_inserted, 0,
        "the children are skipped with the conversation, not inserted under new ids"
    );
    assert_eq!(summary.snippets_inserted, 0);
    assert_eq!(summary.attachments_inserted, 0);

    assert_eq!(count(&pool, "conversation").await?, 1);
    assert_eq!(count(&pool, "message").await?, 1);
    assert_eq!(count(&pool, "snippet").await?, 1);
    assert_eq!(count(&pool, "attachment").await?, 1);

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_inserts_conversation_without_a_hash(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let build = || {
        batch(
            vec![conversation("c1", None)],
            vec![message("m1", "c1", 0)],
            vec![],
            vec![],
        )
    };

    write_batch(&pool, build()).await?;
    // With no hash there is nothing to compare, so a second run has to insert
    // rather than guess. It therefore collides on the primary key instead of
    // silently doing nothing -- the honest failure.
    let error = write_batch(&pool, build())
        .await
        .expect_err("an unhashed conversation cannot be recognised as a duplicate");

    assert!(matches!(error, WriteError::Db(_)), "got {error:?}");

    Ok(())
}

// ---------------------------------------------------------------------------
// Atomicity
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_is_atomic_on_failure(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = batch(
        vec![
            conversation("c1", Some("hash-c1")),
            conversation("c2", Some("hash-c2")),
        ],
        vec![message("m1", "c1", 0), message("m2", "c2", 0)],
        vec![snippet("s1", "m1", "one")],
        vec![
            attachment("a1", "c1", "missing", None, None),
            // Migration 0002's CHECK requires a present attachment to carry
            // both a hash and a blob_path. The normalizer would never emit
            // this, which is the point: the batch is crafted to violate it.
            attachment("a2", "c2", "present", None, None),
        ],
    );

    let error = write_batch(&pool, input)
        .await
        .expect_err("the CHECK constraint should reject the batch");
    assert!(matches!(error, WriteError::Db(_)), "got {error:?}");

    for table in ["conversation", "message", "snippet", "attachment"] {
        assert_eq!(
            count(&pool, table).await?,
            0,
            "{table} should be empty: the first conversation must not survive a batch that failed on the second"
        );
    }

    Ok(())
}

#[sqlx::test(migrations = "../../schema/migrations")]
async fn write_batch_failure_rolls_back_conversations(
    pool: SqlitePool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut orphan = message("m9", "c2", 1);
    // `message.parent_id` references `message(id)`. Pointing it at a row that
    // does not exist fails the insert -- and it fails on the conversation's
    // second message, after the first has already been written inside this
    // transaction.
    orphan.parent_id = Some("does-not-exist".to_owned());

    let input = batch(
        vec![
            conversation("c1", Some("hash-c1")),
            conversation("c2", Some("hash-c2")),
        ],
        vec![message("m1", "c1", 0), message("m8", "c2", 0), orphan],
        vec![],
        vec![],
    );

    let error = write_batch(&pool, input)
        .await
        .expect_err("the dangling parent should fail the insert");
    assert!(matches!(error, WriteError::Db(_)), "got {error:?}");

    assert_eq!(
        count(&pool, "conversation").await?,
        0,
        "the valid conversation from the same batch rolls back with the failing one"
    );
    assert_eq!(count(&pool, "message").await?, 0);

    Ok(())
}
