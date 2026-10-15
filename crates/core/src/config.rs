use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 47821,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub path: String,
    pub use_encryption: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: "~/.kinexfluence/kinexfluence.db".to_string(),
            use_encryption: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
}

#[allow(clippy::derivable_impls)]
impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 47821);
        assert_eq!(config.storage.path, "~/.kinexfluence/kinexfluence.db");
        assert!(config.storage.use_encryption);
    }

    #[test]
    fn config_roundtrips_through_toml() -> anyhow::Result<()> {
        let config = Config::default();
        let serialized = toml::to_string(&config)?;
        let parsed: Config = toml::from_str(&serialized)?;
        assert_eq!(parsed.server.host, config.server.host);
        assert_eq!(parsed.server.port, config.server.port);
        assert_eq!(parsed.storage.path, config.storage.path);
        assert_eq!(parsed.storage.use_encryption, config.storage.use_encryption);
        Ok(())
    }

    #[test]
    fn invalid_port_rejected() {
        let invalid_toml = r#"
            [server]
            host = "127.0.0.1"
            port = 70000

            [storage]
            path = "test.db"
            use_encryption = false
        "#;
        let result: Result<Config, _> = toml::from_str(invalid_toml);
        assert!(result.is_err(), "Port > 65535 should be rejected");
    }
}
