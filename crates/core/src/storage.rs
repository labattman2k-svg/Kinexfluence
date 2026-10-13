use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::Path;
use tracing::debug;

use crate::{KfxResult, StorageConfig};

/// Derives an encryption key from a machine-specific identifier.
/// This is a simple strategy for the MVP; in production, users should
/// be able to provide their own passphrase.
///
/// The key is derived by hashing a combination of:
/// - The machine hostname
/// - The OS type
/// - A fixed application salt
///
/// The derived key is stored in ~/.kinexfluence/key with 0600 permissions.
fn derive_or_load_encryption_key() -> KfxResult<String> {
    let key_path = dirs::home_dir()
        .ok_or_else(|| crate::KfxError::Internal("Cannot determine home directory".to_string()))?
        .join(".kinexfluence")
        .join("key");

    // Try to load existing key
    if let Ok(key) = std::fs::read_to_string(&key_path) {
        debug!("Loaded existing encryption key from {}", key_path.display());
        return Ok(key.trim().to_string());
    }

    // Derive a new key
    let hostname = gethostname::gethostname()
        .to_string_lossy()
        .to_string();
    let os_type = std::env::consts::OS;
    let app_salt = "kinexfluence-v1-salt";

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(os_type.as_bytes());
    hasher.update(app_salt.as_bytes());
    let result = hasher.finalize();

    let hex_key = hex::encode(result);

    // Ensure the key directory exists
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write the key with restrictive permissions
    std::fs::write(&key_path, &hex_key)?;

    // Set file permissions to 0600 (read/write for owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&key_path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&key_path, perms)?;
    }

    debug!("Derived and stored new encryption key at {}", key_path.display());
    Ok(hex_key)
}

pub async fn open(path: &Path, config: &StorageConfig) -> KfxResult<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let url = if config.use_encryption {
        let key = derive_or_load_encryption_key()?;
        format!("sqlite://{}?mode=rwc&_key={}", path.display(), key)
    } else {
        format!("sqlite://{}?mode=rwc", path.display())
    };

    debug!("Opening database at {}", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await?;

    sqlx::query("PRAGMA journal_mode=WAL;").execute(&pool).await?;
    sqlx::query("PRAGMA foreign_keys=ON;").execute(&pool).await?;

    if config.use_encryption {
        debug!("SQLCipher encryption enabled");
    }

    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> KfxResult<()> {
    sqlx::migrate!("../../schema/migrations").run(pool).await?;
    Ok(())
}
