pub mod advanced_types;
/// Shared types and configuration for the CSV Explorer.
///
/// This crate contains all the data types shared across the indexer,
/// storage, API, and UI crates.
pub mod types;

// Re-export commonly used types at the crate root for convenience.
pub use advanced_types::*;
pub use types::*;

// Server-only modules
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub use config::{ApiConfig, ChainConfig, ExplorerConfig};
#[cfg(not(target_arch = "wasm32"))]
pub use error::{ExplorerError, Result};
