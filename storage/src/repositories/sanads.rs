/// Repository for `SanadRecord` operations.
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use csv_explorer_shared::{Result, SanadFilter, SanadRecord, SanadStatus};

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
        let metadata_json = sanad.metadata.as_ref().map(|m| m.to_string());

        sqlx::query(
            r#"
            INSERT INTO sanads (id, chain, seal_ref, commitment, owner, status,
                               created_at, created_tx, transfer_count, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                owner = excluded.owner,
                transfer_count = excluded.transfer_count,
                metadata = excluded.metadata
            "#,
        )
        .bind(&sanad.id)
        .bind(&sanad.chain)
        .bind(&sanad.seal_ref)
        .bind(&sanad.commitment)
        .bind(&sanad.owner)
        .bind(sanad.status.to_string())
        .bind(sanad.created_at.timestamp())
        .bind(&sanad.created_tx)
        .bind(sanad.transfer_count as i64)
        .bind(metadata_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a sanad by ID.
    pub async fn get_by_id(&self, id: &str) -> Result<Option<SanadRecord>> {
        let row = sqlx::query("SELECT * FROM sanads WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| self.row_to_sanad(r)))
    }

    /// List sanads with optional filtering.
    pub async fn list(&self, _filter: &SanadFilter) -> Result<Vec<SanadRecord>> {
        // Basic implementation - filter logic can be added later
        let rows = sqlx::query("SELECT * FROM sanads ORDER BY created_at DESC LIMIT 100")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| self.row_to_sanad(r)).collect())
    }

    /// Count total sanads.
    pub async fn count(&self) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sanads")
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    /// Convert a database row to a SanadRecord.
    fn row_to_sanad(&self, row: SqliteRow) -> SanadRecord {
        use chrono::TimeZone;

        let id: String = row.get("id");
        let chain: String = row.get("chain");
        let seal_ref: String = row.get("seal_ref");
        let commitment: String = row.get("commitment");
        let owner: String = row.get("owner");
        let status_str: String = row.get("status");
        let created_at_ts: i64 = row.get("created_at");
        let created_tx: String = row.get("created_tx");
        let transfer_count: i64 = row.get("transfer_count");
        let metadata_json: Option<String> = row.get("metadata");

        let status = match status_str.as_str() {
            "spent" => SanadStatus::Spent,
            "pending" => SanadStatus::Pending,
            _ => SanadStatus::Active,
        };

        let created_at = match chrono::Utc.timestamp_opt(created_at_ts, 0) {
            chrono::LocalResult::Single(dt) => dt,
            _ => chrono::Utc::now(),
        };

        let metadata = metadata_json.and_then(|s| serde_json::from_str(&s).ok());

        SanadRecord {
            id,
            chain,
            seal_ref,
            commitment,
            owner,
            created_at,
            created_tx,
            status,
            metadata,
            transfer_count: transfer_count as u64,
            last_transfer_at: None,
        }
    }
}
