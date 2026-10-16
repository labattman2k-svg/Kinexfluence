//! CLI tests.
//!
//! The import tests read the database back rather than trusting stdout. A
//! summary line is printed from the `ImportSummary` the writer returned, so
//! asserting on it alone could not tell "wrote seven messages" from "printed
//! the number seven".

use assert_cmd::Command;
use predicates::prelude::*;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// The committed synthetic export from PR #3a.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/chatgpt/sample-export.json")
}

fn import_command(db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("kinexfluence").expect("the binary should build");
    cmd.args([
        "import",
        "chatgpt",
        &fixture().to_string_lossy(),
        "--db",
        &db.to_string_lossy(),
    ]);
    cmd
}

async fn count(db: &Path, table: &str) -> Result<i64, sqlx::Error> {
    let pool = SqlitePool::connect(&format!("sqlite://{}", db.display())).await?;
    let count = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&pool)
        .await?;
    // Deleting the database while the pool is open fails on Windows.
    pool.close().await;
    Ok(count)
}

#[test]
fn migrate_command_creates_db() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::cargo_bin("kinexfluence")?;
    cmd.args(["migrate", "--db", &db_path.to_string_lossy()]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("migrations applied"));

    assert!(
        db_path.exists(),
        "Database file should exist after migration"
    );
    Ok(())
}

#[tokio::test]
async fn import_chatgpt_writes_to_database() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("import.db");

    // Importing into a database that has never been migrated has to work on
    // its own, because that is what a first run looks like.
    import_command(&db_path)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Conversations: 2 inserted, 0 skipped",
        ))
        .stdout(predicate::str::contains("Messages:      7"))
        .stdout(predicate::str::contains("Snippets:      6"))
        .stdout(predicate::str::contains(
            "Attachments:   0 present, 1 missing",
        ));

    assert_eq!(count(&db_path, "source").await?, 1);
    assert_eq!(count(&db_path, "conversation").await?, 2);
    assert_eq!(count(&db_path, "message").await?, 7);
    assert_eq!(count(&db_path, "snippet").await?, 6);
    assert_eq!(count(&db_path, "attachment").await?, 1);

    Ok(())
}

#[tokio::test]
async fn import_chatgpt_twice_is_idempotent() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("import.db");

    import_command(&db_path).assert().success();

    // Every counter is zero on the second run, and the row counts below are
    // what actually proves nothing was written. The source row is the one
    // exception: it is existing data, not a new import.
    import_command(&db_path)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Conversations: 0 inserted, 2 skipped",
        ))
        .stdout(predicate::str::contains("Messages:      0"))
        .stdout(predicate::str::contains("Snippets:      0"))
        .stdout(predicate::str::contains(
            "Attachments:   0 present, 0 missing",
        ));

    assert_eq!(count(&db_path, "conversation").await?, 2);
    assert_eq!(count(&db_path, "message").await?, 7);
    assert_eq!(count(&db_path, "snippet").await?, 6);
    assert_eq!(count(&db_path, "attachment").await?, 1);

    Ok(())
}
