/// Repository for `SealRecord` operations.
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use tuppira_shared::{Result, SealFilter, SealRecord, SealStatus, SealType};

/// Typed repository for the `seals` table.
#[derive(Clone)]
pub struct SealsRepository {
    pool: SqlitePool,
}

impl SealsRepository {
    /// Create a new repository wrapping the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new seal record.
    pub async fn insert(&self, seal: &SealRecord) -> Result<()> {
        let sanad_id = seal.sanad_id.as_deref();
        let consumed_at = seal.consumed_at;
        let consumed_tx = seal.consumed_tx.as_deref();

        sqlx::query(
            r#"
            INSERT INTO seals (id, chain, seal_type, seal_ref, sanad_id, status,
                               consumed_at, consumed_tx, block_height)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                sanad_id = excluded.sanad_id,
                consumed_at = excluded.consumed_at,
                consumed_tx = excluded.consumed_tx
            "#,
        )
        .bind(&seal.id)
        .bind(&seal.chain)
        .bind(seal.seal_type.to_string())
        .bind(&seal.seal_ref)
        .bind(sanad_id)
        .bind(seal.status.to_string())
        .bind(consumed_at)
        .bind(consumed_tx)
        .bind(seal.block_height as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a single seal by ID.
    pub async fn get(&self, id: &str) -> Result<Option<SealRecord>> {
        let row = sqlx::query(
            "SELECT id, chain, seal_type, seal_ref, sanad_id, status, \
             consumed_at, consumed_tx, block_height \
             FROM seals WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_seal(&row)?)),
            None => Ok(None),
        }
    }

    /// List seals matching the given filter.
    pub async fn list(&self, filter: SealFilter) -> Result<Vec<SealRecord>> {
        let mut sql = String::from(
            "SELECT id, chain, seal_type, seal_ref, sanad_id, status, \
             consumed_at, consumed_tx, block_height \
             FROM seals WHERE 1=1",
        );

        if filter.chain.is_some() {
            sql.push_str(" AND chain = ?");
        }
        if filter.seal_type.is_some() {
            sql.push_str(" AND seal_type = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }
        if filter.sanad_id.is_some() {
            sql.push_str(" AND sanad_id = ?");
        }

        sql.push_str(" ORDER BY block_height DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let sql_static = Box::leak(sql.into_boxed_str()) as &'static str;
        let mut query = sqlx::query(sql_static);
        if let Some(ref chain) = filter.chain {
            query = query.bind(chain);
        }
        if let Some(seal_type) = filter.seal_type {
            query = query.bind(seal_type.to_string());
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }
        if let Some(ref sanad_id) = filter.sanad_id {
            query = query.bind(sanad_id);
        }

        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_seal).collect()
    }

    /// Update the status of a seal.
    pub async fn update_status(&self, id: &str, status: SealStatus) -> Result<()> {
        sqlx::query("UPDATE seals SET status = $1 WHERE id = $2")
            .bind(status.to_string())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Count seals matching the filter.
    pub async fn count(&self, filter: SealFilter) -> Result<u64> {
        let mut sql = String::from("SELECT COUNT(*) FROM seals WHERE 1=1");

        if filter.chain.is_some() {
            sql.push_str(" AND chain = ?");
        }
        if filter.seal_type.is_some() {
            sql.push_str(" AND seal_type = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }
        if filter.sanad_id.is_some() {
            sql.push_str(" AND sanad_id = ?");
        }

        // Build the SQL string in a temporary scope to ensure it's not in scope when we create the query
        let sql_static = {
            let mut sql = String::from("SELECT COUNT(*) FROM seals WHERE 1=1");
            if filter.chain.is_some() {
                sql.push_str(" AND chain = ?");
            }
            if filter.seal_type.is_some() {
                sql.push_str(" AND seal_type = ?");
            }
            if filter.status.is_some() {
                sql.push_str(" AND status = ?");
            }
            if filter.sanad_id.is_some() {
                sql.push_str(" AND sanad_id = ?");
            }
            Box::leak(sql.into_boxed_str()) as &'static str
        };
        let mut query = sqlx::query_scalar::<_, i64>(sql_static);
        if let Some(ref chain) = filter.chain {
            query = query.bind(chain);
        }
        if let Some(seal_type) = filter.seal_type {
            query = query.bind(seal_type.to_string());
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }
        if let Some(ref sanad_id) = filter.sanad_id {
            query = query.bind(sanad_id);
        }
        let count = query.fetch_one(&self.pool).await?;

        Ok(count as u64)
    }

    /// Get seals for a specific sanad.
    pub async fn by_sanad(&self, sanad_id: &str) -> Result<Vec<SealRecord>> {
        let rows = sqlx::query(
            "SELECT id, chain, seal_type, seal_ref, sanad_id, status, \
             consumed_at, consumed_tx, block_height \
             FROM seals WHERE sanad_id = ? ORDER BY block_height DESC",
        )
        .bind(sanad_id)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_seal).collect()
    }

    /// Get seals by chain.
    pub async fn by_chain(&self, chain: &str) -> Result<Vec<SealRecord>> {
        let rows = sqlx::query(
            "SELECT id, chain, seal_type, seal_ref, sanad_id, status, \
             consumed_at, consumed_tx, block_height \
             FROM seals WHERE chain = ? ORDER BY block_height DESC",
        )
        .bind(chain)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_seal).collect()
    }

    /// Get a seal by its chain-native identifier (e.g., Bitcoin txid:vout, Sui ObjectId).
    ///
    /// This enables cross-chain seal lookup — given a seal reference from any chain,
    /// you can find the corresponding seal record in the explorer.
    pub async fn get_by_chain_native_id(
        &self,
        chain_native_id: &str,
    ) -> Result<Option<SealRecord>> {
        let row = sqlx::query(
            "SELECT id, chain, seal_type, seal_ref, sanad_id, status, \
             consumed_at, consumed_tx, block_height \
             FROM seals WHERE seal_ref = ?",
        )
        .bind(chain_native_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_seal(&row)?)),
            None => Ok(None),
        }
    }
}

fn row_to_seal(row: &SqliteRow) -> Result<SealRecord> {
    let seal_type_str: String = row.try_get("seal_type")?;
    let seal_type = match seal_type_str.as_str() {
        "utxo" => SealType::Utxo,
        "object" => SealType::Object,
        "resource" => SealType::Resource,
        "nullifier" => SealType::Nullifier,
        "account" => SealType::Account,
        _ => SealType::Utxo,
    };

    let status_str: String = row.try_get("status")?;
    let status = match status_str.as_str() {
        "available" => SealStatus::Available,
        "consumed" => SealStatus::Consumed,
        _ => SealStatus::Available,
    };

    Ok(SealRecord {
        id: row.try_get("id")?,
        chain: row.try_get("chain")?,
        seal_type,
        seal_ref: row.try_get("seal_ref")?,
        sanad_id: row.try_get::<Option<String>, _>("sanad_id")?,
        status,
        consumed_at: row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("consumed_at")?,
        consumed_tx: row.try_get::<Option<String>, _>("consumed_tx")?,
        block_height: row.try_get::<i64, _>("block_height")? as u64,
    })
}
