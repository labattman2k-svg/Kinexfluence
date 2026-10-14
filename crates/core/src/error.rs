use thiserror::Error;

pub type KfxResult<T> = Result<T, KfxError>;

#[derive(Debug, Error)]
pub enum KfxError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("internal: {0}")]
    Internal(String),
}
