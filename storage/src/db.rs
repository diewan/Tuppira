//! Database connection and versioned schema migrations.

use std::{str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use tuppira_shared::{Result, TuppiraError};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_initial.sql")),
    (2, include_str!("../migrations/0002_wallet_feed.sql")),
    (3, include_str!("../migrations/0003_observation_plane.sql")),
    (
        4,
        include_str!("../migrations/0004_reconciliation_history.sql"),
    ),
];

/// Initialize the database connection pool and apply schema.
pub async fn init_pool(database_url: &str, max_connections: u32) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(|error| TuppiraError::Migration(format!("invalid database URL: {error}")))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .connect_with(options)
        .await?;

    apply_migrations(&pool).await?;

    tracing::info!(database_url = %database_url, "Database pool initialized");
    Ok(pool)
}

/// Apply every pending migration atomically. A populated database without the
/// migration ledger is rejected: guessing its schema version is unsafe.
async fn apply_migrations(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _tuppira_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(pool)
    .await?;

    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _tuppira_migrations")
        .fetch_one(pool)
        .await?;
    let has_read_model: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sanads'",
    )
    .fetch_one(pool)
    .await?;
    if applied == 0 && has_read_model != 0 {
        return Err(TuppiraError::Migration(
            "database has explorer tables but no migration ledger; rebuild it from canonical chain data"
                .to_string(),
        ));
    }

    for (version, sql) in MIGRATIONS {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _tuppira_migrations WHERE version = ?")
                .bind(version)
                .fetch_one(pool)
                .await?;
        if exists != 0 {
            continue;
        }

        let mut transaction = pool.begin().await?;
        sqlx::raw_sql(sql).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO _tuppira_migrations (version) VALUES (?)")
            .bind(version)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        tracing::info!(version, "Applied database migration");
    }

    Ok(())
}

/// Gracefully close the connection pool.
pub async fn close_pool(pool: SqlitePool) -> Result<()> {
    pool.close().await;
    tracing::info!("Database pool closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::init_pool;

    #[tokio::test]
    async fn applies_each_migration_once() {
        let pool = init_pool("sqlite::memory:", 1).await;
        assert!(pool.is_ok());
        let pool = match pool {
            Ok(pool) => pool,
            Err(_) => return,
        };
        let count: Result<i64, sqlx::Error> =
            sqlx::query_scalar("SELECT COUNT(*) FROM _tuppira_migrations")
                .fetch_one(&pool)
                .await;
        assert!(matches!(count, Ok(4)));
        let observation_tables: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('observations', 'raw_payload_descriptors', 'collection_runs', 'sync_cursors', 'supersessions')",
        ).fetch_one(&pool).await;
        assert!(matches!(observation_tables, Ok(5)));
        let reconciliation_tables: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('source_reorgs', 'contradiction_hints')",
        ).fetch_one(&pool).await;
        assert!(matches!(reconciliation_tables, Ok(2)));

        // Re-opening the same pool proves the migration ledger is idempotent.
        let reapplied = super::apply_migrations(&pool).await;
        assert!(reapplied.is_ok());
        let count: Result<i64, sqlx::Error> =
            sqlx::query_scalar("SELECT COUNT(*) FROM _tuppira_migrations")
                .fetch_one(&pool)
                .await;
        assert!(matches!(count, Ok(4)));
    }

    #[tokio::test]
    async fn rejects_an_unversioned_read_model() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await;
        assert!(pool.is_ok());
        let pool = match pool {
            Ok(pool) => pool,
            Err(_) => return,
        };
        let created = sqlx::query("CREATE TABLE sanads (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await;
        assert!(created.is_ok());
        let result = super::apply_migrations(&pool).await;
        assert!(matches!(
            result,
            Err(tuppira_shared::TuppiraError::Migration(_))
        ));
    }
}
