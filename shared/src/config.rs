/// Configuration management for the CSV Explorer.
///
/// Provides a unified configuration structure loaded from TOML files
/// and environment variables.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::types::Network;

/// Top-level explorer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerConfig {
    /// Database configuration.
    pub database: DatabaseConfig,
    /// API server configuration.
    pub api: ApiConfig,
    /// UI server configuration.
    pub ui: UiConfig,
    /// Indexer configuration.
    pub indexer: IndexerConfig,
    /// Per-chain configuration.
    pub chains: HashMap<String, ChainConfig>,
}

/// Database connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// SQLite connection string (e.g., "sqlite://explorer.db").
    pub url: String,
    /// Maximum number of connections in the pool.
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

/// API server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Host to bind the API server to.
    #[serde(default = "default_api_host")]
    pub host: String,
    /// Port to listen on.
    #[serde(default = "default_api_port")]
    pub port: u16,
    /// Browser origins permitted to call the public read API. Empty disables
    /// cross-origin browser access rather than allowing every origin.
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Development-only GraphQL explorer and introspection switch.
    #[serde(default)]
    pub enable_graphql_playground: bool,
}

impl ApiConfig {
    /// Returns the full bind address (e.g., "0.0.0.0:8080").
    pub fn bind(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// UI server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Host to bind the UI server to.
    #[serde(default = "default_ui_host")]
    pub host: String,
    /// Port to listen on.
    #[serde(default = "default_ui_port")]
    pub port: u16,
    /// URL of the API server (for frontend fetches).
    #[serde(default = "default_api_url")]
    pub api_url: String,
}

impl UiConfig {
    /// Returns the full bind address.
    pub fn bind(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Indexer daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    /// Number of concurrent chains to sync.
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// Number of blocks to process per batch.
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    /// Poll interval in milliseconds for checking new blocks.
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
}

impl IndexerConfig {
    /// Get the effective poll interval for a chain.
    /// Uses per-chain interval if configured, otherwise falls back to the indexer default.
    pub fn effective_poll_interval(&self, chain_config: &ChainConfig) -> u64 {
        chain_config
            .poll_interval_ms
            .unwrap_or(self.poll_interval_ms)
    }
}

/// Per-chain RPC and indexing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Whether this chain indexer is enabled.
    #[serde(default = "default_chain_enabled")]
    pub enabled: bool,
    /// Network type (mainnet, testnet, devnet).
    pub network: Network,
    /// RPC endpoint URL.
    pub rpc_url: String,
    /// Starting block for initial sync (if not set, starts from genesis).
    pub start_block: Option<u64>,
    /// Per-chain poll interval in milliseconds. Falls back to indexer default if not set.
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

fn default_max_connections() -> u32 {
    5
}

fn default_api_host() -> String {
    "0.0.0.0".to_string()
}

fn default_api_port() -> u16 {
    8080
}

fn default_ui_host() -> String {
    "0.0.0.0".to_string()
}

fn default_ui_port() -> u16 {
    3000
}

fn default_api_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_concurrency() -> usize {
    4
}

fn default_batch_size() -> u64 {
    100
}

fn default_poll_interval() -> u64 {
    5000
}

fn default_chain_enabled() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl ExplorerConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self, crate::ExplorerError> {
        let content = std::fs::read_to_string(path).map_err(crate::ExplorerError::Io)?;
        let config: ExplorerConfig =
            toml::from_str(&content).map_err(|e| crate::ExplorerError::Toml(e.to_string()))?;
        config.with_discovered_chains().with_database_url_from_env()
    }

    /// Load configuration from the default locations.
    ///
    /// Checks (in order):
    /// 1. `CONFIG_PATH` environment variable
    /// 2. `config.toml` in the current directory
    /// 3. Built-in defaults
    pub fn load() -> Result<Self, crate::ExplorerError> {
        if let Ok(path) = std::env::var("CONFIG_PATH") {
            return Self::from_file(Path::new(&path));
        }

        let local = Path::new("config.toml");
        if local.exists() {
            return Self::from_file(local);
        }

        // Fall back to defaults
        Self::default_config()?.with_database_url_from_env()
    }

    /// Create a configuration with all defaults values.
    pub fn default_config() -> Result<Self, crate::ExplorerError> {
        Ok(ExplorerConfig {
            database: DatabaseConfig {
                url: "sqlite://explorer.db".to_string(),
                max_connections: default_max_connections(),
            },
            api: ApiConfig {
                host: default_api_host(),
                port: default_api_port(),
                cors_origins: Vec::new(),
                enable_graphql_playground: false,
            },
            ui: UiConfig {
                host: default_ui_host(),
                port: default_ui_port(),
                api_url: default_api_url(),
            },
            indexer: IndexerConfig {
                concurrency: default_concurrency(),
                batch_size: default_batch_size(),
                poll_interval_ms: default_poll_interval(),
            },
            chains: HashMap::new(),
        }
        .with_discovered_chains())
    }

    /// Merge in chain defaults discovered from the shared `chains/` configuration directory.
    pub fn with_discovered_chains(self) -> Self {
        // TODO: Implement chain discovery from config files
        // For now, return config as-is without chain discovery
        self
    }

    /// Applies the Compose/operator database URL without requiring a second
    /// configuration file. An empty override is a configuration error rather
    /// than a silent fallback to a different database.
    fn with_database_url_from_env(self) -> Result<Self, crate::ExplorerError> {
        self.with_database_url_override(std::env::var("DATABASE_URL").ok())
    }

    fn with_database_url_override(
        mut self,
        database_url: Option<String>,
    ) -> Result<Self, crate::ExplorerError> {
        if let Some(url) = database_url {
            if url.trim().is_empty() {
                return Err(crate::ExplorerError::Parse(
                    "DATABASE_URL must not be empty".to_string(),
                ));
            }
            self.database.url = url;
        }
        Ok(self)
    }
}

#[allow(dead_code)]
fn parse_network(network: &str) -> Network {
    match network.to_ascii_lowercase().as_str() {
        "test" | "testnet" | "sepolia" => Network::Testnet,
        "dev" | "devnet" | "regtest" => Network::Devnet,
        _ => Network::Mainnet,
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        DatabaseConfig {
            url: "sqlite://explorer.db".to_string(),
            max_connections: default_max_connections(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        ApiConfig {
            host: default_api_host(),
            port: default_api_port(),
            cors_origins: Vec::new(),
            enable_graphql_playground: false,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            host: default_ui_host(),
            port: default_ui_port(),
            api_url: default_api_url(),
        }
    }
}

impl Default for IndexerConfig {
    fn default() -> Self {
        IndexerConfig {
            concurrency: default_concurrency(),
            batch_size: default_batch_size(),
            poll_interval_ms: default_poll_interval(),
        }
    }
}

impl Default for ChainConfig {
    fn default() -> Self {
        ChainConfig {
            enabled: default_chain_enabled(),
            network: Network::Mainnet,
            rpc_url: String::new(),
            start_block: None,
            poll_interval_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExplorerConfig;

    #[test]
    fn database_url_override_replaces_file_or_default_configuration() {
        let result = ExplorerConfig::default_config().and_then(|config| {
            config.with_database_url_override(Some("sqlite:///data/explorer.db".to_string()))
        });
        assert!(
            matches!(result, Ok(config) if config.database.url == "sqlite:///data/explorer.db")
        );
    }

    #[test]
    fn empty_database_url_override_fails_closed() {
        let result = ExplorerConfig::default_config()
            .and_then(|config| config.with_database_url_override(Some("   ".to_string())));
        assert!(
            matches!(result, Err(crate::ExplorerError::Parse(message)) if message == "DATABASE_URL must not be empty")
        );
    }
}
