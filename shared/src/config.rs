/// Configuration management for the Tuppira.
///
/// Provides a unified configuration structure loaded from TOML files
/// and environment variables.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::types::Network;
use csv_sdk::rpc_policy::{ChainRpcPolicy, RpcCapability};

/// Top-level explorer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuppiraConfig {
    /// Database configuration.
    pub database: DatabaseConfig,
    /// API server configuration.
    pub api: ApiConfig,
    /// Indexer configuration.
    pub indexer: IndexerConfig,
    /// Per-chain configuration.
    pub chains: HashMap<String, ChainConfig>,
}

/// Database connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// SQLite connection string (e.g., "sqlite://tuppira.db").
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
    /// Canonical endpoint and trust policy for this chain.
    pub rpc_policy: ChainRpcPolicy,
    /// Transitional projection used by current indexers until RPC-002 moves
    /// every adapter to capability-based endpoint resolution. This is never
    /// read from or written to profile TOML.
    #[serde(skip)]
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

impl TuppiraConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self, crate::TuppiraError> {
        let content = std::fs::read_to_string(path).map_err(crate::TuppiraError::Io)?;
        let config: TuppiraConfig =
            toml::from_str(&content).map_err(|e| crate::TuppiraError::Toml(e.to_string()))?;
        config
            .with_resolved_rpc_urls()?
            .with_discovered_chains()
            .with_database_url_from_env()
    }

    /// Load configuration from the default locations.
    ///
    /// Checks (in order):
    /// 1. `CONFIG_PATH` environment variable
    /// 2. `config.toml` in the current directory
    /// 3. Built-in defaults
    pub fn load() -> Result<Self, crate::TuppiraError> {
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
    pub fn default_config() -> Result<Self, crate::TuppiraError> {
        Ok(TuppiraConfig {
            database: DatabaseConfig {
                url: "sqlite://tuppira.db".to_string(),
                max_connections: default_max_connections(),
            },
            api: ApiConfig {
                host: default_api_host(),
                port: default_api_port(),
                cors_origins: Vec::new(),
                enable_graphql_playground: false,
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

    /// Validate each profile policy and derive the legacy request URL from its
    /// explicit read-capable endpoint. No URL fallback or transport guessing is
    /// allowed during this migration.
    fn with_resolved_rpc_urls(mut self) -> Result<Self, crate::TuppiraError> {
        for (chain, config) in &mut self.chains {
            config
                .rpc_policy
                .validate()
                .map_err(|error| crate::TuppiraError::Parse(error.to_string()))?;
            if config.rpc_policy.chain != *chain {
                return Err(crate::TuppiraError::Parse(format!(
                    "chain profile key {chain} does not match policy chain {}",
                    config.rpc_policy.chain
                )));
            }
            let endpoint = config
                .rpc_policy
                .candidates(RpcCapability::Read)
                .map_err(|error| crate::TuppiraError::Parse(error.to_string()))?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    crate::TuppiraError::Parse(format!(
                        "chain {chain} policy has no read-capable endpoint"
                    ))
                })?;
            config.rpc_url = endpoint.url.clone();
        }
        Ok(self)
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
    fn with_database_url_from_env(self) -> Result<Self, crate::TuppiraError> {
        self.with_database_url_override(std::env::var("DATABASE_URL").ok())
    }

    fn with_database_url_override(
        mut self,
        database_url: Option<String>,
    ) -> Result<Self, crate::TuppiraError> {
        if let Some(url) = database_url {
            if url.trim().is_empty() {
                return Err(crate::TuppiraError::Parse(
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
            url: "sqlite://tuppira.db".to_string(),
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
            rpc_policy: ChainRpcPolicy {
                chain: String::new(),
                network: String::new(),
                selection: Default::default(),
                endpoints: Vec::new(),
            },
            rpc_url: String::new(),
            start_block: None,
            poll_interval_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TuppiraConfig;
    use std::path::Path;

    fn tuppira_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("shared crate lives under tuppira")
            .to_path_buf()
    }

    #[test]
    fn database_url_override_replaces_file_or_default_configuration() {
        let result = TuppiraConfig::default_config().and_then(|config| {
            config.with_database_url_override(Some("sqlite:///data/tuppira.db".to_string()))
        });
        assert!(matches!(result, Ok(config) if config.database.url == "sqlite:///data/tuppira.db"));
    }

    #[test]
    fn empty_database_url_override_fails_closed() {
        let result = TuppiraConfig::default_config()
            .and_then(|config| config.with_database_url_override(Some("   ".to_string())));
        assert!(
            matches!(result, Err(crate::TuppiraError::Parse(message)) if message == "DATABASE_URL must not be empty")
        );
    }

    #[test]
    fn every_shipped_tuppira_profile_uses_a_valid_canonical_rpc_policy() {
        for profile in [
            "config.toml",
            "config.mainnet.toml",
            "config.testnet.toml",
            "config.example.toml",
        ] {
            let path = tuppira_root().join(profile);
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{profile} must be readable: {error}"));
            assert!(
                !contents.contains("rpc_url") && !contents.contains("${"),
                "{profile} must not contain scalar or shell-expanded RPC configuration"
            );
            let config: TuppiraConfig = toml::from_str(&contents)
                .unwrap_or_else(|error| panic!("{profile} must deserialize: {error}"));
            let config = config
                .with_resolved_rpc_urls()
                .unwrap_or_else(|error| panic!("{profile} policy must validate: {error}"));
            assert_eq!(
                config.chains.len(),
                5,
                "{profile} must configure five chains"
            );
            assert!(
                config
                    .chains
                    .values()
                    .all(|chain| !chain.rpc_url.is_empty()),
                "{profile} must derive every legacy request bridge from policy"
            );
        }
    }
}
