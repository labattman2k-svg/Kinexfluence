use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

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

#[test]
fn import_command_reports_not_yet_wired() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let dummy_export = temp_dir.path().join("export.json");
    std::fs::write(&dummy_export, "{}")?;

    let mut cmd = Command::cargo_bin("kinexfluence")?;
    cmd.args(["import", "chatgpt", &dummy_export.to_string_lossy()]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("not yet wired"));

    Ok(())
}
