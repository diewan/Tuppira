/// Error types for the CSV Explorer.
use thiserror::Error;

/// Top-level error type for the explorer.
#[derive(Error, Debug)]
pub enum ExplorerError {
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

impl ExplorerError {
    pub fn error_code(&self) -> &'static str {
        match self {
            ExplorerError::Io(_) => "EXP_IO_ERROR",
            ExplorerError::Toml(_) => "EXP_TOML_ERROR",
            ExplorerError::Json(_) => "EXP_JSON_ERROR",
            ExplorerError::Database(_) => "EXP_DATABASE_ERROR",
            ExplorerError::Migration(_) => "EXP_MIGRATION_ERROR",
            ExplorerError::NotFound { .. } => "EXP_ENTITY_NOT_FOUND",
            ExplorerError::Http(_) => "EXP_HTTP_ERROR",
            ExplorerError::RpcError { .. } => "EXP_RPC_ERROR",
            ExplorerError::RpcParseError { .. } => "EXP_RPC_PARSE_ERROR",
            ExplorerError::IndexerStopped => "EXP_INDEXER_STOPPED",
            ExplorerError::BlockError { .. } => "EXP_BLOCK_ERROR",
            ExplorerError::ChainReorg { .. } => "EXP_CHAIN_REORG",
            ExplorerError::GraphQL(_) => "EXP_GRAPHQL_ERROR",
            ExplorerError::HttpServer(_) => "EXP_HTTP_SERVER_ERROR",
            ExplorerError::Hex(_) => "EXP_HEX_DECODE_ERROR",
            ExplorerError::Parse(_) => "EXP_PARSE_ERROR",
            ExplorerError::Internal(_) => "EXP_INTERNAL_ERROR",
        }
    }

    pub fn description(&self) -> String {
        self.to_string()
    }

    pub fn suggested_fix(&self) -> String {
        match self {
            ExplorerError::Io(_) => {
                "I/O operation failed. Check file permissions and disk space.".to_string()
            }
            ExplorerError::Toml(_) => {
                "TOML configuration parsing failed. Check syntax in config files.".to_string()
            }
            ExplorerError::Json(_) => {
                "JSON parsing failed. Check API responses are valid JSON.".to_string()
            }
            ExplorerError::Database(_) => {
                "Database operation failed. Check connection and schema.".to_string()
            }
            ExplorerError::Migration(_) => {
                "Database migration failed. Check migration files and database state.".to_string()
            }
            ExplorerError::NotFound { entity_type, id } => {
                format!("{} '{}' not found in database.", entity_type, id)
            }
            ExplorerError::Http(_) => {
                "HTTP request failed. Check network and API endpoints.".to_string()
            }
            ExplorerError::RpcError { chain, .. } => {
                format!("RPC error on chain {}. Check node status and retry.", chain)
            }
            ExplorerError::RpcParseError { chain, .. } => {
                format!(
                    "RPC parse error on {}. Response format may have changed.",
                    chain
                )
            }
            ExplorerError::IndexerStopped => {
                "Indexer has stopped. Check logs and restart.".to_string()
            }
            ExplorerError::BlockError { chain, block, .. } => {
                format!(
                    "Error processing block {} on {}. Check block validity.",
                    block, chain
                )
            }
            ExplorerError::ChainReorg {
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
            ExplorerError::GraphQL(_) => {
                "GraphQL operation failed. Check query syntax and schema.".to_string()
            }
            ExplorerError::HttpServer(_) => {
                "HTTP server error. Check server configuration and ports.".to_string()
            }
            ExplorerError::Hex(_) => {
                "Hex decoding failed. Check input is valid hexadecimal.".to_string()
            }
            ExplorerError::Parse(_) => {
                "Parse error. Check input format matches expected type.".to_string()
            }
            ExplorerError::Internal(_) => {
                "Internal error. Check logs for details and report if persistent.".to_string()
            }
        }
    }

    pub fn docs_url(&self) -> String {
        format!("https://docs.csv.dev/errors/{}", self.error_code())
    }
}

pub type Result<T> = std::result::Result<T, ExplorerError>;
