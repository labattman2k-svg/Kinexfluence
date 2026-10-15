//! ChatGPT export parsing.
//!
//! Scope is set by ADR 0007: export-file import only, no live DOM capture.

pub mod parser;
pub mod types;

pub use parser::{ChatGptAdapter, EXPORT_SHAPE};
