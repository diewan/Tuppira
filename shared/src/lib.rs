pub mod advanced_types;
/// Shared types and configuration for the Tuppira.
///
/// This crate contains all the data types shared across the indexer,
/// storage, and API crates.
pub mod types;

/// Official block-explorer link construction for the chains Parwana touches.
pub mod block_explorer;

/// Source-neutral observation-plane projections and validation.
pub mod observation;

/// Observation-plane projections of Parwana V2 source closure.
pub mod closure_observation;

// Re-export commonly used types at the crate root for convenience.
pub use advanced_types::*;
pub use closure_observation::*;
pub use observation::*;
pub use types::*;

// Server-only modules
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub use config::{ApiConfig, ChainConfig, TuppiraConfig};
#[cfg(not(target_arch = "wasm32"))]
pub use error::{Result, TuppiraError};
