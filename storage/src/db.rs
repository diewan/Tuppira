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
    (
        5,
        include_str!("../migrations/0005_accountable_entities.sql"),
    ),
    (
        6,
        include_str!("../migrations/0006_closure_observations.sql"),
    ),
    (
        7,
        include_str!("../migrations/0007_closure_chain_evidence.sql"),
    ),
    (
        8,
        include_str!("../migrations/0008_closure_reorg_standing.sql"),
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
        assert!(matches!(count, Ok(8)));
        let observation_tables: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('observations', 'raw_payload_descriptors', 'collection_runs', 'sync_cursors', 'supersessions')",
        ).fetch_one(&pool).await;
        assert!(matches!(observation_tables, Ok(5)));
        let reconciliation_tables: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('source_reorgs', 'contradiction_hints')",
        ).fetch_one(&pool).await;
        assert!(matches!(reconciliation_tables, Ok(2)));
        let closure_tables: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('closure_observations', 'closure_observation_evidence', 'closure_observation_evidence_refs')",
        ).fetch_one(&pool).await;
        assert!(matches!(closure_tables, Ok(3)));
        let reorg_standing_tables: Result<i64, sqlx::Error> = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('closure_observation_orphanings', 'closure_observation_orphaning_reasons', 'closure_index_tips')",
        ).fetch_one(&pool).await;
        assert!(matches!(reorg_standing_tables, Ok(3)));

        // Re-opening the same pool proves the migration ledger is idempotent.
        let reapplied = super::apply_migrations(&pool).await;
        assert!(reapplied.is_ok());
        let count: Result<i64, sqlx::Error> =
            sqlx::query_scalar("SELECT COUNT(*) FROM _tuppira_migrations")
                .fetch_one(&pool)
                .await;
        assert!(matches!(count, Ok(8)));
    }

    /// The V1 explorer read model must survive the closure migration untouched:
    /// a Sanad written before it keeps every column, and `status = 'spent'`
    /// still means what it meant — a chain-level spend the indexer saw, not a
    /// V2 protocol closure.
    #[tokio::test]
    async fn the_closure_migration_leaves_pre_migration_history_intact() {
        let Ok(pool) = sqlx::SqlitePool::connect("sqlite::memory:").await else {
            return;
        };
        // Apply every migration up to but excluding the closure migration, then
        // write history the way a pre-closure release would have.
        assert!(
            sqlx::query(
                "CREATE TABLE _tuppira_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
            )
            .execute(&pool)
            .await
            .is_ok()
        );
        for (version, sql) in &super::MIGRATIONS[..5] {
            assert!(sqlx::raw_sql(sql).execute(&pool).await.is_ok());
            assert!(
                sqlx::query("INSERT INTO _tuppira_migrations (version) VALUES (?)")
                    .bind(version)
                    .execute(&pool)
                    .await
                    .is_ok()
            );
        }
        for statement in [
            "INSERT INTO sanads (id, chain, seal_ref, commitment, owner, created_at, created_tx, status, transfer_count) \
             VALUES ('sanad:legacy', 'ethereum', 'seal:1', 'commit:1', 'owner:1', 1, 'tx:1', 'spent', 2)",
            "INSERT INTO transfers (id, sanad_id, from_chain, to_chain, from_owner, to_owner, lock_tx, status, created_at) \
             VALUES ('transfer:legacy', 'sanad:legacy', 'ethereum', 'sui', 'owner:1', 'owner:2', 'tx:lock', 'completed', 2)",
            "INSERT INTO seals (id, chain, seal_type, seal_ref, sanad_id, status, consumed_tx, block_height) \
             VALUES ('seal:legacy', 'ethereum', 'utxo', 'seal:1', 'sanad:legacy', 'consumed', 'tx:consume', 900)",
        ] {
            assert!(sqlx::query(statement).execute(&pool).await.is_ok());
        }

        assert!(super::apply_migrations(&pool).await.is_ok());

        let sanad: Result<(String, String, i64), sqlx::Error> =
            sqlx::query_as("SELECT status, created_tx, transfer_count FROM sanads WHERE id = 'sanad:legacy'")
                .fetch_one(&pool)
                .await;
        assert_eq!(sanad.ok(), Some(("spent".into(), "tx:1".into(), 2)));
        let transfer: Result<(String, String), sqlx::Error> =
            sqlx::query_as("SELECT status, lock_tx FROM transfers WHERE id = 'transfer:legacy'")
                .fetch_one(&pool)
                .await;
        assert_eq!(transfer.ok(), Some(("completed".into(), "tx:lock".into())));
        let seal: Result<(String, String, i64), sqlx::Error> =
            sqlx::query_as("SELECT status, consumed_tx, block_height FROM seals WHERE id = 'seal:legacy'")
                .fetch_one(&pool)
                .await;
        assert_eq!(
            seal.ok(),
            Some(("consumed".into(), "tx:consume".into(), 900))
        );
        // The migration attaches no closure statement to that history.
        let attached: Result<i64, sqlx::Error> =
            sqlx::query_scalar("SELECT COUNT(*) FROM closure_observations")
                .fetch_one(&pool)
                .await;
        assert!(matches!(attached, Ok(0)));
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
