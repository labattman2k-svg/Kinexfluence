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
             data is stored in plaintext. See ADR 0005."
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
