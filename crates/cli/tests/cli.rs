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

// ---------------------------------------------------------------------------
// project
// ---------------------------------------------------------------------------

/// Run a `project` subcommand against a temp database.
fn project_command(db: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::cargo_bin("kinexfluence").expect("the binary should build");
    cmd.arg("project");
    cmd.args(args);
    cmd.args(["--db", &db.to_string_lossy()]);
    cmd
}

#[test]
fn project_create_and_list() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("project.db");

    let created = project_command(
        &db_path,
        &[
            "create",
            "Test Project",
            "--brief",
            "Testing the project CLI",
        ],
    )
    .output()?;

    assert!(created.status.success(), "create should succeed");
    let id = String::from_utf8(created.stdout)?.trim().to_owned();

    // Scriptability is the point: stdout is the id and nothing else, so
    // `PID=$(kinexfluence project create ...)` works.
    assert_eq!(id.len(), 36, "stdout should be one uuid, got {id:?}");
    assert_eq!(id.matches('-').count(), 4, "got {id:?}");
    assert!(
        String::from_utf8(created.stderr)?.contains("test-project"),
        "the human-readable line belongs on stderr"
    );

    let listed = project_command(&db_path, &["list"]).output()?;
    assert!(listed.status.success());
    let table = String::from_utf8(listed.stdout)?;

    // `list` resolves from the database, so the id appearing here is evidence
    // the row exists -- not just that `create` echoed something.
    assert!(
        table.contains(&id),
        "list output should contain the id:\n{table}"
    );
    assert!(table.contains("test-project"), "and the slug:\n{table}");
    assert!(table.contains("Test Project"), "and the name:\n{table}");
    assert!(table.contains("recon"), "and the status:\n{table}");

    Ok(())
}

#[test]
fn project_show_by_slug() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("project.db");

    project_command(
        &db_path,
        &[
            "create",
            "Test Project",
            "--brief",
            "Testing the project CLI",
        ],
    )
    .assert()
    .success();

    // By slug, not id: the subcommand has to resolve either.
    let shown = project_command(&db_path, &["show", "test-project"]).output()?;
    assert!(shown.status.success());

    let detail = String::from_utf8(shown.stdout)?;
    assert!(detail.contains("Project: Test Project"), "got:\n{detail}");
    assert!(
        detail.contains("Slug:        test-project"),
        "got:\n{detail}"
    );
    assert!(detail.contains("Status:      recon"), "got:\n{detail}");
    assert!(
        detail.contains("Brief:       Testing the project CLI"),
        "the brief should be shown, got:\n{detail}"
    );

    Ok(())
}

#[test]
fn project_create_rejects_empty_name() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("project.db");

    let rejected = project_command(&db_path, &["create", ""]).output()?;

    assert_eq!(
        rejected.status.code(),
        Some(2),
        "invalid input exits 2, per the shared convention"
    );
    let stderr = String::from_utf8(rejected.stderr)?;
    assert!(
        stderr.contains("invalid input"),
        "the error should say what went wrong, got:\n{stderr}"
    );
    assert!(rejected.stdout.is_empty(), "nothing should reach stdout");

    Ok(())
}

// ---------------------------------------------------------------------------
// cart
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cart_add_and_list() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("cart.db");

    // Import test data
    import_command(&db_path).assert().success();

    // Create a project
    let created = project_command(
        &db_path,
        &["create", "Test Project", "--brief", "Testing cart"],
    )
    .output()?;
    assert!(created.status.success());
    let project_id = String::from_utf8(created.stdout)?.trim().to_owned();

    // Get a snippet ID from the database
    let pool = SqlitePool::connect(&format!("sqlite://{}", db_path.display())).await?;
    let snippet_id: String = sqlx::query_scalar("SELECT id FROM snippet LIMIT 1")
        .fetch_one(&pool)
        .await?;
    pool.close().await;

    // Add to cart
    let mut cmd = Command::cargo_bin("kinexfluence")?;
    cmd.args([
        "cart",
        "add",
        &snippet_id,
        "--project",
        &project_id,
        "--db",
        &db_path.to_string_lossy(),
    ]);
    cmd.assert().success();

    // List cart
    let mut cmd = Command::cargo_bin("kinexfluence")?;
    cmd.args([
        "cart",
        "list",
        "--project",
        &project_id,
        "--db",
        &db_path.to_string_lossy(),
    ]);
    let output = cmd.output()?;
    assert!(output.status.success());
    let table = String::from_utf8(output.stdout)?;
    assert!(!table.is_empty(), "cart should not be empty after add");

    Ok(())
}
