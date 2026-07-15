/// Repository for `CsvContract` operations.
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use tuppira_shared::{ContractFilter, ContractStatus, ContractType, CsvContract, Result};

/// Typed repository for the `contracts` table.
#[derive(Clone)]
pub struct ContractsRepository {
    pool: SqlitePool,
}

impl ContractsRepository {
    /// Create a new repository wrapping the given pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new contract record.
    pub async fn insert(&self, contract: &CsvContract) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO contracts (id, chain, contract_type, address, deployed_tx,
                                   deployed_at, version, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                version = excluded.version
            "#,
        )
        .bind(&contract.id)
        .bind(&contract.chain)
        .bind(contract.contract_type.to_string())
        .bind(&contract.address)
        .bind(&contract.deployed_tx)
        .bind(contract.deployed_at)
        .bind(&contract.version)
        .bind(contract.status.to_string())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get a single contract by ID.
    pub async fn get(&self, id: &str) -> Result<Option<CsvContract>> {
        let row = sqlx::query(
            "SELECT id, chain, contract_type, address, deployed_tx, deployed_at, \
             version, status FROM contracts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_contract(&row)?)),
            None => Ok(None),
        }
    }

    /// List contracts matching the given filter.
    pub async fn list(&self, filter: ContractFilter) -> Result<Vec<CsvContract>> {
        let mut sql = String::from(
            "SELECT id, chain, contract_type, address, deployed_tx, deployed_at, \
             version, status FROM contracts WHERE 1=1",
        );

        if filter.chain.is_some() {
            sql.push_str(" AND chain = ?");
        }
        if filter.contract_type.is_some() {
            sql.push_str(" AND contract_type = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }

        sql.push_str(" ORDER BY deployed_at DESC");

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
        if let Some(contract_type) = filter.contract_type {
            query = query.bind(contract_type.to_string());
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }

        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_contract).collect()
    }

    /// Count contracts matching the filter.
    pub async fn count(&self, filter: ContractFilter) -> Result<u64> {
        // Build the SQL string in a temporary scope to ensure it's not in scope when we create the query
        let sql_static = {
            let mut sql = String::from("SELECT COUNT(*) FROM contracts WHERE 1=1");
            if filter.chain.is_some() {
                sql.push_str(" AND chain = ?");
            }
            if filter.contract_type.is_some() {
                sql.push_str(" AND contract_type = ?");
            }
            if filter.status.is_some() {
                sql.push_str(" AND status = ?");
            }
            Box::leak(sql.into_boxed_str()) as &'static str
        };
        let mut query = sqlx::query_scalar::<_, i64>(sql_static);
        if let Some(ref chain) = filter.chain {
            query = query.bind(chain);
        }
        if let Some(contract_type) = filter.contract_type {
            query = query.bind(contract_type.to_string());
        }
        if let Some(status) = filter.status {
            query = query.bind(status.to_string());
        }
        let count = query.fetch_one(&self.pool).await?;

        Ok(count as u64)
    }

    /// Get contracts by chain.
    pub async fn by_chain(&self, chain: &str) -> Result<Vec<CsvContract>> {
        let rows = sqlx::query(
            "SELECT id, chain, contract_type, address, deployed_tx, deployed_at, \
             version, status FROM contracts WHERE chain = ? ORDER BY deployed_at DESC",
        )
        .bind(chain)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_contract).collect()
    }
}

fn row_to_contract(row: &SqliteRow) -> Result<CsvContract> {
    let contract_type_str: String = row.try_get("contract_type")?;
    let contract_type = match contract_type_str.as_str() {
        "nullifier_registry" => ContractType::NullifierRegistry,
        "state_commitment" => ContractType::StateCommitment,
        "sanad_registry" => ContractType::SanadRegistry,
        "bridge" => ContractType::Bridge,
        _ => ContractType::Other,
    };

    let status_str: String = row.try_get("status")?;
    let status = match status_str.as_str() {
        "active" => ContractStatus::Active,
        "deprecated" => ContractStatus::Deprecated,
        "error" => ContractStatus::Error,
        _ => ContractStatus::Active,
    };

    Ok(CsvContract {
        id: row.try_get("id")?,
        chain: row.try_get("chain")?,
        contract_type,
        address: row.try_get("address")?,
        deployed_tx: row.try_get("deployed_tx")?,
        deployed_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("deployed_at")?,
        version: row.try_get("version")?,
        status,
    })
}
