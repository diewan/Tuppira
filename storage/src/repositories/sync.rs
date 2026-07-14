/// Repository for sync progress tracking.
use chrono::Utc;
use sqlx::SqlitePool;

use csv_explorer_shared::Result;

/// Repository for managing chain sync progress.
#[derive(Clone)]
pub struct SyncRepository {
    pool: SqlitePool,
}

impl SyncRepository {
    /// Create a new repository wrapping the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get the latest synced block for a chain.
    pub async fn get_latest_block(&self, chain: &str) -> Result<Option<u64>> {
        let row =
            sqlx::query_scalar::<_, i64>("SELECT latest_block FROM sync_progress WHERE chain = $1")
                .bind(chain)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(|v| v as u64))
    }

    /// Get the latest synced slot for a chain (if applicable).
    pub async fn get_latest_slot(&self, chain: &str) -> Result<Option<u64>> {
        let row: Option<Option<i64>> =
            sqlx::query_scalar("SELECT latest_slot FROM sync_progress WHERE chain = $1")
                .bind(chain)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.flatten().map(|v| v as u64))
    }

    /// Update the sync progress for a chain.
    pub async fn update_progress(
        &self,
        chain: &str,
        latest_block: u64,
        latest_slot: Option<u64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO sync_progress (chain, latest_block, latest_slot, last_synced_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT(chain) DO UPDATE SET
                latest_block = excluded.latest_block,
                latest_slot = excluded.latest_slot,
                last_synced_at = excluded.last_synced_at
            "#,
        )
        .bind(chain)
        .bind(latest_block as i64)
        .bind(latest_slot.map(|v| v as i64))
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all chain sync progress entries.
    pub async fn get_all(&self) -> Result<Vec<SyncProgressRow>> {
        let rows = sqlx::query_as::<_, SyncProgressRow>(
            "SELECT chain, latest_block, latest_slot, last_synced_at FROM sync_progress",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Reset sync progress for a chain.
    pub async fn reset(&self, chain: &str) -> Result<()> {
        sqlx::query("DELETE FROM sync_progress WHERE chain = $1")
            .bind(chain)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Reset all sync progress.
    pub async fn reset_all(&self) -> Result<()> {
        sqlx::query("DELETE FROM sync_progress")
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

/// Raw row type for sync_progress table.
#[derive(Debug, sqlx::FromRow)]
pub struct SyncProgressRow {
    pub chain: String,
    pub latest_block: i64,
    pub latest_slot: Option<i64>,
    pub last_synced_at: Option<chrono::DateTime<chrono::Utc>>,
}
