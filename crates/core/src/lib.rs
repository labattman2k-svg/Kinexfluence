//! Kinexfluence core: storage, normalizer, scorer, probe engine, exporter.
pub mod api;
pub mod cart;
pub mod config;
pub mod embeddings;
pub mod error;
pub mod normalizer;
pub mod probe;
pub mod project;
pub mod scorer;
pub mod server;
pub mod snippet;
pub mod storage;
pub mod writer;

pub use config::{Config, ServerConfig, StorageConfig};
pub use error::{KfxError, KfxResult};
