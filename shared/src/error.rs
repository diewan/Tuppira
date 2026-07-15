/// Error types for the Tuppira.
use thiserror::Error;

/// Top-level error type for the explorer.
#[derive(Error, Debug)]
pub enum TuppiraError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML error: {0}")]
    Toml(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(String),

    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("RPC error on chain {chain}: {message}")]
    RpcError { chain: String, message: String },

    #[error("RPC parse error on chain {chain}: {message}")]
    RpcParseError { chain: String, message: String },

    #[error("Indexer stopped")]
    IndexerStopped,

    #[error("Block processing error on chain {chain} at block {block}: {message}")]
    BlockError {
        chain: String,
        block: u64,
        message: String,
    },

    #[error("Chain reorg detected on {chain} at block {block}, depth: {depth}")]
    ChainReorg {
        chain: String,
        block: u64,
        depth: u64,
    },

    #[error("GraphQL error: {0}")]
    GraphQL(String),

    #[error("HTTP server error: {0}")]
    HttpServer(String),

    #[error("Hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl TuppiraError {
    pub fn error_code(&self) -> &'static str {
        match self {
            TuppiraError::Io(_) => "EXP_IO_ERROR",
            TuppiraError::Toml(_) => "EXP_TOML_ERROR",
            TuppiraError::Json(_) => "EXP_JSON_ERROR",
            TuppiraError::Database(_) => "EXP_DATABASE_ERROR",
            TuppiraError::Migration(_) => "EXP_MIGRATION_ERROR",
            TuppiraError::NotFound { .. } => "EXP_ENTITY_NOT_FOUND",
            TuppiraError::Http(_) => "EXP_HTTP_ERROR",
            TuppiraError::RpcError { .. } => "EXP_RPC_ERROR",
            TuppiraError::RpcParseError { .. } => "EXP_RPC_PARSE_ERROR",
            TuppiraError::IndexerStopped => "EXP_INDEXER_STOPPED",
            TuppiraError::BlockError { .. } => "EXP_BLOCK_ERROR",
            TuppiraError::ChainReorg { .. } => "EXP_CHAIN_REORG",
            TuppiraError::GraphQL(_) => "EXP_GRAPHQL_ERROR",
            TuppiraError::HttpServer(_) => "EXP_HTTP_SERVER_ERROR",
            TuppiraError::Hex(_) => "EXP_HEX_DECODE_ERROR",
            TuppiraError::Parse(_) => "EXP_PARSE_ERROR",
            TuppiraError::Internal(_) => "EXP_INTERNAL_ERROR",
        }
    }

    pub fn description(&self) -> String {
        self.to_string()
    }

    pub fn suggested_fix(&self) -> String {
        match self {
            TuppiraError::Io(_) => {
                "I/O operation failed. Check file permissions and disk space.".to_string()
            }
            TuppiraError::Toml(_) => {
                "TOML configuration parsing failed. Check syntax in config files.".to_string()
            }
            TuppiraError::Json(_) => {
                "JSON parsing failed. Check API responses are valid JSON.".to_string()
            }
            TuppiraError::Database(_) => {
                "Database operation failed. Check connection and schema.".to_string()
            }
            TuppiraError::Migration(_) => {
                "Database migration failed. Check migration files and database state.".to_string()
            }
            TuppiraError::NotFound { entity_type, id } => {
                format!("{} '{}' not found in database.", entity_type, id)
            }
            TuppiraError::Http(_) => {
                "HTTP request failed. Check network and API endpoints.".to_string()
            }
            TuppiraError::RpcError { chain, .. } => {
                format!("RPC error on chain {}. Check node status and retry.", chain)
            }
            TuppiraError::RpcParseError { chain, .. } => {
                format!(
                    "RPC parse error on {}. Response format may have changed.",
                    chain
                )
            }
            TuppiraError::IndexerStopped => {
                "Indexer has stopped. Check logs and restart.".to_string()
            }
            TuppiraError::BlockError { chain, block, .. } => {
                format!(
                    "Error processing block {} on {}. Check block validity.",
                    block, chain
                )
            }
            TuppiraError::ChainReorg {
                chain,
                block,
                depth,
            } => {
                format!(
                    "Reorg detected on {} at block {} (depth {}). \
                     May need to re-index affected blocks.",
                    chain, block, depth
                )
            }
            TuppiraError::GraphQL(_) => {
                "GraphQL operation failed. Check query syntax and schema.".to_string()
            }
            TuppiraError::HttpServer(_) => {
                "HTTP server error. Check server configuration and ports.".to_string()
            }
            TuppiraError::Hex(_) => {
                "Hex decoding failed. Check input is valid hexadecimal.".to_string()
            }
            TuppiraError::Parse(_) => {
                "Parse error. Check input format matches expected type.".to_string()
            }
            TuppiraError::Internal(_) => {
                "Internal error. Check logs for details and report if persistent.".to_string()
            }
        }
    }

    pub fn docs_url(&self) -> String {
        format!("https://docs.csv.dev/errors/{}", self.error_code())
    }
}

pub type Result<T> = std::result::Result<T, TuppiraError>;
