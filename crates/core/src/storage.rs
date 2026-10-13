use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::Path;
use tracing::debug;

use crate::{KfxResult, StorageConfig};

pub async fn open(path: &Path, config: &StorageConfig) -> KfxResult<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // TODO: Implement SQLCipher encryption (ADR 0003)
    // For now, we use standard SQLite. SQLCipher integration requires:
    // 1. Building SQLx against SQLCipher libraries
    // 2. Key derivation strategy (ADR 0005)
    // 3. Testing FTS5 and sqlite-vec compatibility
    if config.use_encryption {
        debug!("Encryption requested but not yet implemented - using standard SQLite");
    }

    let url = format!("sqlite://{}?mode=rwc", path.display());

    debug!("Opening database at {}", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await?;

    sqlx::query("PRAGMA journal_mode=WAL;").execute(&pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON;").execute(&pool).await?;

    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> KfxResult<()> {
    sqlx::migrate!("../../schema/migrations").run(pool).await?;
    Ok(())
}
