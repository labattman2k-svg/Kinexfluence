use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCapture {
    pub source: String,
    pub conversation: serde_json::Value,
    pub messages: Vec<serde_json::Value>,
    pub attachments: Vec<serde_json::Value>,
}

pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn matches_url(&self, url: &str) -> bool;
    fn parse_export(&self, bytes: &[u8]) -> anyhow::Result<RawCapture>;
}
