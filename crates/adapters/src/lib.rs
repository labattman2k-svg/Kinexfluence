//! Provider adapters: one module per source (ChatGPT, Claude, Gemini, ...).
pub mod chatgpt;
pub mod provider;

pub use provider::{AdapterError, ProviderAdapter, RawCapture};
