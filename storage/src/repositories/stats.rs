/// Repository for aggregate statistics queries.
use sqlx::SqlitePool;

use csv_explorer_shared::{ChainCount, ChainPairCount, ExplorerStats, Result};

/// Repository for aggregate statistics.
#[derive(Clone)]
pub struct StatsRepository {
    pool: SqlitePool,
}

impl StatsRepository {
    /// Create a new repository wrapping the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get full aggregate statistics.
    pub async fn get_stats(&self) -> Result<ExplorerStats> {
        let total_sanads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sanads")
            .fetch_one(&self.pool)
            .await?;

        let total_transfers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transfers")
            .fetch_one(&self.pool)
            .await?;

        let total_seals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM seals")
            .fetch_one(&self.pool)
            .await?;

        let total_contracts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contracts")
            .fetch_one(&self.pool)
            .await?;

        // Sanads by chain
        let sanads_by_chain_rows = sqlx::query_as::<_, ChainCountRow>(
            "SELECT chain, COUNT(*) as count FROM sanads GROUP BY chain ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let sanads_by_chain = sanads_by_chain_rows
            .into_iter()
            .map(|r| ChainCount {
                chain: r.chain,
                count: r.count as u64,
            })
            .collect();

        // Transfers by chain pair
        let transfers_by_pair_rows = sqlx::query_as::<_, ChainPairCountRow>(
            "SELECT from_chain, to_chain, COUNT(*) as count \
             FROM transfers GROUP BY from_chain, to_chain ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let transfers_by_chain_pair = transfers_by_pair_rows
            .into_iter()
            .map(|r| ChainPairCount {
                from_chain: r.from_chain,
                to_chain: r.to_chain,
                count: r.count as u64,
            })
            .collect();

        // Active seals by chain
        let active_seals_rows = sqlx::query_as::<_, ChainCountRow>(
            "SELECT chain, COUNT(*) as count FROM seals \
             WHERE status = 'available' GROUP BY chain ORDER BY count DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let active_seals_by_chain = active_seals_rows
            .into_iter()
            .map(|r| ChainCount {
                chain: r.chain,
                count: r.count as u64,
            })
            .collect();

        // Transfer success rate
        let completed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transfers WHERE status = 'completed'")
                .fetch_one(&self.pool)
                .await?;

        let failed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM transfers WHERE status = 'failed'")
                .fetch_one(&self.pool)
                .await?;

        let total_finished = completed + failed;
        let transfer_success_rate = if total_finished > 0 {
            (completed as f64 / total_finished as f64) * 100.0
        } else {
            100.0
        };

        // Average transfer time (for completed transfers)
        let avg_transfer_time: Option<f64> = sqlx::query_scalar(
            "SELECT AVG(duration_ms) FROM transfers WHERE status = 'completed' AND duration_ms IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(ExplorerStats {
            total_sanads: total_sanads as u64,
            total_transfers: total_transfers as u64,
            total_seals: total_seals as u64,
            total_contracts: total_contracts as u64,
            sanads_by_chain,
            transfers_by_chain_pair,
            active_seals_by_chain,
            transfer_success_rate,
            average_transfer_time_ms: avg_transfer_time.map(|v| v.round() as u64),
        })
    }

    /// Get total sanads count.
    pub async fn total_sanads(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sanads")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }

    /// Get total transfers count.
    pub async fn total_transfers(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transfers")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }

    /// Get total seals count.
    pub async fn total_seals(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM seals")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }

    /// Get total contracts count.
    pub async fn total_contracts(&self) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contracts")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ChainCountRow {
    chain: String,
    count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ChainPairCountRow {
    from_chain: String,
    to_chain: String,
    count: i64,
}
