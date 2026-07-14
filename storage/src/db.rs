use sqlx::SqlitePool;
/// Database connection pool and initialization.
///
/// Provides a typed wrapper around `SqlitePool` with automatic schema
/// application on first connect.
use sqlx::sqlite::SqlitePoolOptions;
use std::time::Duration;

use csv_explorer_shared::Result;

/// The raw SQL schema to apply on first run.
const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Initialize the database connection pool and apply schema.
pub async fn init_pool(database_url: &str, max_connections: u32) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .connect(database_url)
        .await?;

    apply_schema(&pool).await?;

    tracing::info!(database_url = %database_url, "Database pool initialized");
    Ok(pool)
}

/// Apply the schema DDL if tables do not yet exist.
async fn apply_schema(pool: &SqlitePool) -> Result<()> {
    // Check if the sanads table exists; if not, apply the full schema.
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sanads'",
    )
    .fetch_one(pool)
    .await?;

    if exists == Some(0) {
        tracing::info!("Applying database schema");
        // Execute each statement from the schema file separately
        // (sqlite doesn't support multi-statement execution via query_scalar)
        for statement in SCHEMA_SQL.split(';') {
            let trimmed = statement.trim();
            if !trimmed.is_empty() {
                sqlx::query(trimmed).execute(pool).await?;
            }
        }
        tracing::info!("Database schema applied successfully");
    } else {
        tracing::debug!("Database schema already exists, skipping");
    }

    Ok(())
}

/// Gracefully close the connection pool.
pub async fn close_pool(pool: SqlitePool) -> Result<()> {
    pool.close().await;
    tracing::info!("Database pool closed");
    Ok(())
}
