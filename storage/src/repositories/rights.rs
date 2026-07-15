/// Repository for `SanadRecord` operations.
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use tuppira_shared::{Result, SanadFilter, SanadRecord, SanadStatus};

/// Typed repository for the `sanads` table.
#[derive(Clone)]
pub struct SanadsRepository {
    pool: SqlitePool,
}

impl SanadsRepository {
    /// Create a new repository wrapping the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new sanad record.
    pub async fn insert(&self, sanad: &SanadRecord) -> Result<()> {
        let metadata_json = sanad
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let last_transfer_at = sanad.last_transfer_at;

        sqlx::query(
            r#"
            INSERT INTO sanads (id, chain, seal_ref, commitment, owner, created_at, created_tx,
                                status, metadata, transfer_count, last_transfer_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                owner = excluded.owner,
                transfer_count = excluded.transfer_count,
                last_transfer_at = excluded.last_transfer_at,
                metadata = excluded.metadata
            "#,
        )
        .bind(&sanad.id)
        .bind(&sanad.chain)
        .bind(&sanad.seal_ref)
        .bind(&sanad.commitment)
        .bind(&sanad.owner)
        .bind(sanad.created_at)
        .bind(&sanad.created_tx)
        .bind(sanad.status.to_string())
        .bind(metadata_json)
        .bind(sanad.transfer_count as i64)
        .bind(last_transfer_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a single sanad by ID.
    pub async fn get(&self, id: &str) -> Result<Option<SanadRecord>> {
        let row = sqlx::query(
            r#"SELECT id, chain, seal_ref, commitment, owner, created_at, created_tx,
                      status, metadata, transfer_count, last_transfer_at
               FROM sanads WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_sanad(&row)?)),
            None => Ok(None),
        }
    }

    /// List sanads matching the given filter.
    pub async fn list(&self, filter: SanadFilter) -> Result<Vec<SanadRecord>> {
        let mut sql = String::from(
            "SELECT id, chain, seal_ref, commitment, owner, created_at, created_tx, \
             status, metadata, transfer_count, last_transfer_at FROM sanads WHERE 1=1",
        );
        let mut records = Vec::new();

        if filter.chain.is_some() {
            sql.push_str(" AND chain = ?");
        }
        if filter.owner.is_some() {
            sql.push_str(" AND owner = ?");
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
        if let Some(ref chain) = filter.chain {
            query = query.bind(chain);
        }
        if let Some(ref owner) = filter.owner {
            query = query.bind(owner);
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }

        let rows = query.fetch_all(&self.pool).await?;
        for row in rows {
            records.push(row_to_sanad(&row)?);
        }

        Ok(records)
    }

    /// Update the status of a sanad.
    pub async fn update_status(&self, id: &str, status: SanadStatus) -> Result<()> {
        sqlx::query("UPDATE sanads SET status = $1 WHERE id = $2")
            .bind(status.to_string())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Count sanads matching the filter.
    pub async fn count(&self, filter: SanadFilter) -> Result<u64> {
        let mut sql = String::from("SELECT COUNT(*) FROM sanads WHERE 1=1");

        if filter.chain.is_some() {
            sql.push_str(" AND chain = ?");
        }
        if filter.owner.is_some() {
            sql.push_str(" AND owner = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }

        let mut query = sqlx::query_scalar::<_, i64>(&sql);
        if let Some(ref chain) = filter.chain {
            query = query.bind(chain);
        }
        if let Some(ref owner) = filter.owner {
            query = query.bind(owner);
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }

        let count = query.fetch_one(&self.pool).await?;
        Ok(count as u64)
    }

    /// Get all sanads owned by a specific address.
    pub async fn by_owner(&self, owner: &str) -> Result<Vec<SanadRecord>> {
        let rows = sqlx::query(
            "SELECT id, chain, seal_ref, commitment, owner, created_at, created_tx, \
             status, metadata, transfer_count, last_transfer_at \
             FROM sanads WHERE owner = ? ORDER BY created_at DESC",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_sanad).collect()
    }

    /// Get all sanads on a specific chain.
    pub async fn by_chain(&self, chain: &str) -> Result<Vec<SanadRecord>> {
        let rows = sqlx::query(
            "SELECT id, chain, seal_ref, commitment, owner, created_at, created_tx, \
             status, metadata, transfer_count, last_transfer_at \
             FROM sanads WHERE chain = ? ORDER BY created_at DESC",
        )
        .bind(chain)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_sanad).collect()
    }

    /// Get recently created sanads.
    pub async fn recent(&self, limit: usize) -> Result<Vec<SanadRecord>> {
        let rows = sqlx::query(
            "SELECT id, chain, seal_ref, commitment, owner, created_at, created_tx, \
             status, metadata, transfer_count, last_transfer_at \
             FROM sanads ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_sanad).collect()
    }
}

fn row_to_sanad(row: &SqliteRow) -> Result<SanadRecord> {
    let status_str: String = row.try_get("status")?;
    let status = match status_str.as_str() {
        "active" => SanadStatus::Active,
        "spent" => SanadStatus::Spent,
        "pending" => SanadStatus::Pending,
        _ => SanadStatus::Active,
    };

    let metadata: Option<serde_json::Value> = row
        .try_get::<Option<String>, _>("metadata")?
        .map(|s: String| serde_json::from_str(&s))
        .transpose()?;

    Ok(SanadRecord {
        id: row.try_get("id")?,
        chain: row.try_get("chain")?,
        seal_ref: row.try_get("seal_ref")?,
        commitment: row.try_get("commitment")?,
        owner: row.try_get("owner")?,
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        created_tx: row.try_get("created_tx")?,
        status,
        metadata,
        transfer_count: row.try_get::<i64, _>("transfer_count")? as u64,
        last_transfer_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_transfer_at")?,
    })
}
