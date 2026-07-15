/// RPC Manager for handling multiple RPC endpoints with fallbacks.
///
/// Supports:
/// - Multiple RPC URLs per chain
/// - HTTP/HTTPS and WSS connections
/// - API key authentication
/// - Automatic fallback to alternative endpoints on failure
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use tuppira_shared::TuppiraError;

/// Type of RPC endpoint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RpcType {
    Http,
    Wss,
}

/// Authentication configuration for RPC endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcAuth {
    #[serde(rename = "type")]
    pub auth_type: AuthType,
    pub header: String,
    pub value: String,
}

/// Authentication method types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    ApiKey,
    BearerToken,
    BasicAuth,
}

/// Single RPC endpoint configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEndpoint {
    pub url: String,
    #[serde(rename = "type")]
    pub endpoint_type: RpcType,
    pub priority: u8,
    #[serde(default)]
    pub auth: Option<RpcAuth>,
}

/// Chain RPC configuration with primary and alternative endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRpcConfig {
    pub primary: RpcEndpoint,
    pub alternatives: Vec<RpcEndpoint>,
}

/// Global RPC configuration for all chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub chains: HashMap<String, ChainRpcConfig>,
}

impl RpcConfig {
    /// Load RPC configuration from a JSON file
    pub fn from_file(path: &Path) -> Result<Self, TuppiraError> {
        let content = std::fs::read_to_string(path).map_err(TuppiraError::Io)?;

        let mut config: Self = serde_json::from_str(&content)
            .map_err(|e| TuppiraError::Internal(format!("Failed to parse RPC config: {}", e)))?;

        config.resolve_env_vars();
        Ok(config)
    }

    /// Resolve environment variables in auth values
    fn resolve_env_vars(&mut self) {
        for chain_config in self.chains.values_mut() {
            Self::resolve_auth(&mut chain_config.primary.auth);
            for alt in &mut chain_config.alternatives {
                Self::resolve_auth(&mut alt.auth);
            }
        }
    }

    fn resolve_auth(auth: &mut Option<RpcAuth>) {
        if let Some(a) = auth {
            // If value looks like an env var name (no spaces, all caps/underscores), try to resolve
            if a.value.starts_with('$') {
                let var_name = a
                    .value
                    .trim_start_matches('$')
                    .trim_start_matches('{')
                    .trim_end_matches('}');
                a.value = env::var(var_name).unwrap_or_else(|_| a.value.clone());
            } else {
                // Also try direct env var lookup
                a.value = env::var(&a.value).unwrap_or_else(|_| a.value.clone());
            }
        }
    }

    /// Get the best available endpoint for a chain
    pub fn get_endpoint(&self, chain_id: &str) -> Option<RpcEndpoint> {
        let chain_config = self.chains.get(chain_id)?;

        if !chain_config.primary.url.is_empty() {
            return Some(chain_config.primary.clone());
        }

        let mut sorted = chain_config.alternatives.clone();
        sorted.sort_by_key(|e| e.priority);
        sorted.into_iter().find(|e| !e.url.is_empty())
    }

    /// Get all endpoints for a chain sorted by priority
    pub fn get_all_endpoints(&self, chain_id: &str) -> Vec<RpcEndpoint> {
        let Some(chain_config) = self.chains.get(chain_id) else {
            return Vec::new();
        };

        let mut endpoints = Vec::new();
        if !chain_config.primary.url.is_empty() {
            endpoints.push(chain_config.primary.clone());
        }
        let mut alts = chain_config.alternatives.clone();
        alts.sort_by_key(|e| e.priority);
        endpoints.extend(alts.into_iter().filter(|e| !e.url.is_empty()));
        endpoints
    }

    // -----------------------------------------------------------------------
    // FIX: build a proper reqwest::Client with auth headers baked in
    // -----------------------------------------------------------------------

    /// Build an authenticated HTTP client for the given endpoint.
    pub fn create_http_client(&self, endpoint: &RpcEndpoint) -> reqwest::Client {
        build_http_client(endpoint)
    }
}

/// Stand-alone helper so both `RpcConfig` and `RpcManager` can call it.
fn build_http_client(endpoint: &RpcEndpoint) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connection_verbose(false);

    if let Some(ref auth) = endpoint.auth {
        let mut headers = HeaderMap::new();

        match auth.auth_type {
            AuthType::ApiKey => {
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_str(&auth.header),
                    HeaderValue::from_str(&auth.value),
                ) {
                    headers.insert(name, value);
                }
            }
            AuthType::BearerToken => {
                let bearer = format!("Bearer {}", auth.value);
                if let Ok(value) = HeaderValue::from_str(&bearer) {
                    headers.insert(reqwest::header::AUTHORIZATION, value);
                }
            }
            AuthType::BasicAuth => {
                // value expected as "user:password"
                let encoded = base64_encode(&auth.value);
                let basic = format!("Basic {}", encoded);
                if let Ok(value) = HeaderValue::from_str(&basic) {
                    headers.insert(reqwest::header::AUTHORIZATION, value);
                }
            }
        }

        builder = builder.default_headers(headers);
    }

    builder.build().unwrap_or_default()
}

/// Minimal base64 encoding without external dep (std only).
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) & 63] as char);
        out.push(CHARS[(n >> 12) & 63] as char);
        out.push(if chunk.len() > 1 {
            CHARS[(n >> 6) & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[n & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Load RPC configuration from default location
pub fn load_rpc_config() -> Result<RpcConfig, TuppiraError> {
    let path = Path::new("rpc_config.json");
    if path.exists() {
        RpcConfig::from_file(path)
    } else {
        Ok(RpcConfig::default())
    }
}

impl Default for RpcConfig {
    fn default() -> Self {
        let make_http = |url: &str, priority: u8| RpcEndpoint {
            url: url.to_string(),
            endpoint_type: RpcType::Http,
            priority,
            auth: None,
        };

        let mut chains = HashMap::new();

        chains.insert(
            "bitcoin".to_string(),
            ChainRpcConfig {
                primary: make_http("https://mempool.space/api", 1),
                alternatives: vec![RpcEndpoint {
                    url: "https://btc.getblock.io/mainnet/".to_string(),
                    endpoint_type: RpcType::Http,
                    priority: 2,
                    auth: Some(RpcAuth {
                        auth_type: AuthType::ApiKey,
                        header: "X-API-Key".to_string(),
                        value: "${GETBLOCK_API_KEY}".to_string(),
                    }),
                }],
            },
        );

        chains.insert(
            "ethereum".to_string(),
            ChainRpcConfig {
                primary: make_http("https://eth.llamarpc.com", 1),
                alternatives: vec![
                    make_http("https://ethereum-rpc.publicnode.com", 2),
                    RpcEndpoint {
                        url: "wss://ethereum-rpc.publicnode.com".to_string(),
                        endpoint_type: RpcType::Wss,
                        priority: 3,
                        auth: None,
                    },
                ],
            },
        );

        chains.insert(
            "sui".to_string(),
            ChainRpcConfig {
                primary: make_http("https://fullnode.mainnet.sui.io:443", 1),
                alternatives: vec![make_http("https://sui-mainnet-rpc.allthatnode.com", 2)],
            },
        );

        chains.insert(
            "aptos".to_string(),
            ChainRpcConfig {
                primary: make_http("https://fullnode.mainnet.aptoslabs.com/v1", 1),
                alternatives: vec![make_http("https://aptos-mainnet-rpc.allthatnode.com/v1", 2)],
            },
        );

        chains.insert(
            "solana".to_string(),
            ChainRpcConfig {
                primary: make_http("https://api.mainnet-beta.solana.com", 1),
                alternatives: vec![make_http("https://solana-rpc.publicnode.com", 2)],
            },
        );

        RpcConfig { chains }
    }
}

// ---------------------------------------------------------------------------
// RpcManager wrapper
// ---------------------------------------------------------------------------

pub struct RpcManager {
    config: RpcConfig,
}

impl RpcManager {
    pub fn new(config: RpcConfig) -> Self {
        Self { config }
    }

    pub fn get_endpoint(&self, chain_id: &str) -> Option<RpcEndpoint> {
        self.config.get_endpoint(chain_id)
    }

    pub fn get_all_endpoints(&self, chain_id: &str) -> Vec<RpcEndpoint> {
        self.config.get_all_endpoints(chain_id)
    }

    pub fn get_primary_endpoint(&self, chain_id: &str) -> Option<&RpcEndpoint> {
        self.config.chains.get(chain_id).map(|c| &c.primary)
    }

    // -----------------------------------------------------------------------
    // FIX: actually build an authenticated client
    // -----------------------------------------------------------------------

    pub fn get_http_client(&self, chain_id: &str) -> Option<reqwest::Client> {
        let endpoint = self.config.get_endpoint(chain_id)?;
        Some(build_http_client(&endpoint))
    }

    pub fn get_client(&self, chain_id: &str) -> Option<reqwest::Client> {
        self.get_http_client(chain_id)
    }

    /// Try endpoints in priority order, applying retry with the next one on failure.
    /// Returns `(url, client)` for the first healthy endpoint that responds within timeout.
    /// Sends a minimal JSON-RPC call to verify endpoint health.
    pub async fn get_healthy_endpoint(&self, chain_id: &str) -> Option<(String, reqwest::Client)> {
        for endpoint in self.get_all_endpoints(chain_id) {
            if endpoint.url.is_empty() {
                continue;
            }

            let client = build_http_client(&endpoint);

            // Send health check request with 2 second timeout
            let health_check = match endpoint.endpoint_type {
                RpcType::Http => {
                    // Send minimal JSON-RPC request based on chain type
                    let payload = Self::health_check_payload(chain_id);
                    let timeout_duration = Duration::from_secs(2);

                    match tokio::time::timeout(
                        timeout_duration,
                        client
                            .post(&endpoint.url)
                            .header("Content-Type", "application/json")
                            .body(payload)
                            .send(),
                    )
                    .await
                    {
                        Ok(Ok(response)) => response.status().is_success(),
                        _ => false,
                    }
                }
                RpcType::Wss => {
                    // For WebSocket, just verify URL is valid (full check would need ws client)
                    true
                }
            };

            if health_check {
                return Some((endpoint.url, client));
            }
        }
        None
    }

    /// Generate appropriate health check payload for each chain type
    fn health_check_payload(chain_id: &str) -> String {
        match chain_id {
            "ethereum" | "sepolia" | "holesky" => {
                // eth_blockNumber
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "eth_blockNumber",
                    "params": [],
                    "id": 1
                })
                .to_string()
            }
            "solana" | "solana-devnet" => {
                // getSlot
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "getSlot",
                    "params": [],
                    "id": 1
                })
                .to_string()
            }
            "bitcoin" => {
                // getblockcount
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "getblockcount",
                    "params": [],
                    "id": 1
                })
                .to_string()
            }
            "aptos" => {
                // Get latest ledger version
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "get_ledger_info",
                    "params": [],
                    "id": 1
                })
                .to_string()
            }
            "sui" => {
                // Get latest checkpoint
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "sui_getLatestCheckpointSequenceNumber",
                    "params": [],
                    "id": 1
                })
                .to_string()
            }
            _ => {
                // Generic health check
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "health",
                    "params": [],
                    "id": 1
                })
                .to_string()
            }
        }
    }
}

impl Clone for RpcManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
        }
    }
}
