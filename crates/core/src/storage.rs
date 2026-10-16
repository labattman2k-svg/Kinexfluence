use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::Path;
use tracing::{debug, warn};

use crate::{KfxResult, StorageConfig};

pub async fn open(path: &Path, config: &StorageConfig) -> KfxResult<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if config.use_encryption {
        warn!(
            target: "kfx.storage",
            "use_encryption=true is set but SQLCipher is not yet wired; \
             data is stored in plaintext. See ADR 0006."
        );
    }

    let url = format!("sqlite://{}?mode=rwc", path.display());

    debug!("Opening database at {}", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await?;

    sqlx::query("PRAGMA journal_mode=WAL;")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys=ON;")
        .execute(&pool)
        .await?;

    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> KfxResult<()> {
    sqlx::migrate!("../../schema/migrations").run(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn open_creates_parent_dirs() -> anyhow::Result<()> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("nested").join("dir").join("test.db");
        let config = StorageConfig {
            path: db_path.to_string_lossy().to_string(),
            use_encryption: false,
        };

        let pool = open(&db_path, &config).await?;
        assert!(
            db_path.parent().is_some_and(|p| p.exists()),
            "Parent directory should be created"
        );
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn migrate_is_idempotent() -> anyhow::Result<()> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let config = StorageConfig {
            path: db_path.to_string_lossy().to_string(),
            use_encryption: false,
        };

        let pool = open(&db_path, &config).await?;
        migrate(&pool).await?;
        migrate(&pool).await?; // Second run should not error
        pool.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn migrate_reports_schema_version() -> anyhow::Result<()> {
        let temp_dir = TempDir::new()?;
        let db_path = temp_dir.path().join("test.db");
        let config = StorageConfig {
            path: db_path.to_string_lossy().to_string(),
            use_encryption: false,
        };

        let pool = open(&db_path, &config).await?;
        migrate(&pool).await?;

        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM _sqlx_migrations ORDER BY version DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await?;

        // Derived from the embedded migrator rather than hardcoded to a
        // literal. Hardcoding it broke the moment migration 0002 was added,
        // and the failure was invisible locally: `sqlx::migrate!` emits one
        // `include_str!` per file it found when the macro last ran, and a new
        // file in the directory is not a tracked input, so cargo does not
        // re-run the macro. This crate kept embedding only 0001 and the old
        // assertion passed against a stale list while CI, compiling from
        // scratch, failed. See docs/migrations.md.
        let expected = sqlx::migrate!("../../schema/migrations")
            .iter()
            .map(|migration| migration.version)
            .max()
            .expect("there should be at least one migration");

        assert_eq!(
            version, expected,
            "the highest applied migration should be the last one embedded"
        );
        pool.close().await;
        Ok(())
    }
}
