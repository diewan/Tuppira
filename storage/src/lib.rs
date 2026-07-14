/// Storage layer for the CSV Explorer.
///
/// Provides a typed repository pattern over SQLite for all indexed data,
/// including sanads, transfers, seals, contracts, sync progress, and statistics.
#[cfg(target_arch = "wasm32")]
compile_error!("csv-explorer-storage requires native platform (SQLite/sqlx not available on wasm32)");

pub mod db;
pub mod repositories;

pub use db::{close_pool, init_pool};
