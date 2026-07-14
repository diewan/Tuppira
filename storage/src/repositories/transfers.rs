/// Repository for `TransferRecord` operations.
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use csv_explorer_shared::{Result, TransferFilter, TransferRecord, TransferStatus};

/// Typed repository for the `transfers` table.
#[derive(Clone)]
pub struct TransfersRepository {
    pool: SqlitePool,
}

impl TransfersRepository {
    /// Create a new repository wrapping the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new transfer record.
    pub async fn insert(&self, transfer: &TransferRecord) -> Result<()> {
        let mint_tx = transfer.mint_tx.as_deref();
        let proof_ref = transfer.proof_ref.as_deref();
        let completed_at = transfer.completed_at;

        sqlx::query(
            r#"
            INSERT INTO transfers (id, sanad_id, from_chain, to_chain, from_owner, to_owner,
                                   lock_tx, mint_tx, proof_ref, status, created_at, completed_at,
                                   duration_ms)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                mint_tx = excluded.mint_tx,
                proof_ref = excluded.proof_ref,
                completed_at = excluded.completed_at,
                duration_ms = excluded.duration_ms,
                to_owner = excluded.to_owner
            "#,
        )
        .bind(&transfer.id)
        .bind(&transfer.sanad_id)
        .bind(&transfer.from_chain)
        .bind(&transfer.to_chain)
        .bind(&transfer.from_owner)
        .bind(&transfer.to_owner)
        .bind(&transfer.lock_tx)
        .bind(mint_tx)
        .bind(proof_ref)
        .bind(transfer.status.to_string())
        .bind(transfer.created_at)
        .bind(completed_at)
        .bind(transfer.duration_ms.map(|v| v as i64))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a single transfer by ID.
    pub async fn get(&self, id: &str) -> Result<Option<TransferRecord>> {
        let row = sqlx::query(
            "SELECT id, sanad_id, from_chain, to_chain, from_owner, to_owner, \
             lock_tx, mint_tx, proof_ref, status, created_at, completed_at, duration_ms \
             FROM transfers WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_transfer(&row)?)),
            None => Ok(None),
        }
    }

    /// List transfers matching the given filter.
    pub async fn list(&self, filter: TransferFilter) -> Result<Vec<TransferRecord>> {
        let mut sql = String::from(
            "SELECT id, sanad_id, from_chain, to_chain, from_owner, to_owner, \
             lock_tx, mint_tx, proof_ref, status, created_at, completed_at, duration_ms \
             FROM transfers WHERE 1=1",
        );

        if filter.sanad_id.is_some() {
            sql.push_str(" AND sanad_id = ?");
        }
        if filter.from_chain.is_some() {
            sql.push_str(" AND from_chain = ?");
        }
        if filter.to_chain.is_some() {
            sql.push_str(" AND to_chain = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let sql_static = Box::leak(sql.into_boxed_str()) as &'static str;
        let mut query = sqlx::query(sql_static);
        if let Some(ref sanad_id) = filter.sanad_id {
            query = query.bind(sanad_id);
        }
        if let Some(ref from_chain) = filter.from_chain {
            query = query.bind(from_chain);
        }
        if let Some(ref to_chain) = filter.to_chain {
            query = query.bind(to_chain);
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }

        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_transfer).collect()
    }

    /// Update the status of a transfer.
    pub async fn update_status(&self, id: &str, status: TransferStatus) -> Result<()> {
        sqlx::query("UPDATE transfers SET status = $1 WHERE id = $2")
            .bind(status.to_string())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Count transfers matching the filter.
    pub async fn count(&self, filter: TransferFilter) -> Result<u64> {
        let mut sql = String::from("SELECT COUNT(*) FROM transfers WHERE 1=1");

        if filter.sanad_id.is_some() {
            sql.push_str(" AND sanad_id = ?");
        }
        if filter.from_chain.is_some() {
            sql.push_str(" AND from_chain = ?");
        }
        if filter.to_chain.is_some() {
            sql.push_str(" AND to_chain = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }

        // Build the SQL string in a temporary scope to ensure it's not in scope when we create the query
        let sql_static = {
            let mut sql = String::from("SELECT COUNT(*) FROM transfers WHERE 1=1");
            if filter.sanad_id.is_some() {
                sql.push_str(" AND sanad_id = ?");
            }
            if filter.from_chain.is_some() {
                sql.push_str(" AND from_chain = ?");
            }
            if filter.to_chain.is_some() {
                sql.push_str(" AND to_chain = ?");
            }
            if filter.status.is_some() {
                sql.push_str(" AND status = ?");
            }
            Box::leak(sql.into_boxed_str()) as &'static str
        };
        let mut query = sqlx::query_scalar::<_, i64>(sql_static);
        if let Some(ref sanad_id) = filter.sanad_id {
            query = query.bind(sanad_id);
        }
        if let Some(ref from_chain) = filter.from_chain {
            query = query.bind(from_chain);
        }
        if let Some(ref to_chain) = filter.to_chain {
            query = query.bind(to_chain);
        }
        if let Some(ref status) = filter.status {
            query = query.bind(status.to_string());
        }
        let count = query.fetch_one(&self.pool).await?;

        Ok(count as u64)
    }

    /// Get transfers for a specific sanad.
    pub async fn by_sanad(&self, sanad_id: &str) -> Result<Vec<TransferRecord>> {
        let rows = sqlx::query(
            "SELECT id, sanad_id, from_chain, to_chain, from_owner, to_owner, \
             lock_tx, mint_tx, proof_ref, status, created_at, completed_at, duration_ms \
             FROM transfers WHERE sanad_id = ? ORDER BY created_at DESC",
        )
        .bind(sanad_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_transfer).collect()
    }

    /// Get recent transfers.
    pub async fn recent(&self, limit: usize) -> Result<Vec<TransferRecord>> {
        let rows = sqlx::query(
            "SELECT id, sanad_id, from_chain, to_chain, from_owner, to_owner, \
             lock_tx, mint_tx, proof_ref, status, created_at, completed_at, duration_ms \
             FROM transfers ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_transfer).collect()
    }
}

fn row_to_transfer(row: &SqliteRow) -> Result<TransferRecord> {
    let status_str: String = row.try_get("status")?;
    let status = match status_str.as_str() {
        "pending" | "initiated" => TransferStatus::Initiated,
        "in_progress" | "locking" | "submitting_proof" | "verifying" | "minting" => {
            TransferStatus::SubmittingProof
        }
        "completed" => TransferStatus::Completed,
        "failed" => TransferStatus::Failed {
            error_code: "UNKNOWN".to_string(),
            retryable: true,
        },
        _ => TransferStatus::Initiated,
    };

    Ok(TransferRecord {
        id: row.try_get("id")?,
        sanad_id: row.try_get("sanad_id")?,
        from_chain: row.try_get("from_chain")?,
        to_chain: row.try_get("to_chain")?,
        from_owner: row.try_get("from_owner")?,
        to_owner: row.try_get("to_owner")?,
        lock_tx: row.try_get("lock_tx")?,
        mint_tx: row.try_get::<Option<String>, _>("mint_tx")?,
        proof_ref: row.try_get::<Option<String>, _>("proof_ref")?,
        status,
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        completed_at: row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at")?,
        duration_ms: row
            .try_get::<Option<i64>, _>("duration_ms")?
            .map(|v| v as u64),
        lock_tx_explorer_url: None,
        mint_tx_explorer_url: None,
    })
}
