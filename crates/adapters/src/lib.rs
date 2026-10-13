//! Provider adapters: one module per source (ChatGPT, Claude, Gemini, ...).
pub mod provider;

pub use provider::{ProviderAdapter, RawCapture};
