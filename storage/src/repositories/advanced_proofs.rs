/// Repository for advanced commitment and proof metadata.
///
/// Handles storage and querying of commitment schemes, proof types,
/// and enhanced sanad/seal records with metadata.
use tuppira_shared::{
    CommitmentScheme, EnhancedInclusionProof, EnhancedSanadRecord, EnhancedSealRecord,
    EnhancedTransferRecord, FinalityProofCount, FinalityProofType, InclusionProofCount,
    InclusionProofType, ProofStatistics, ProofVerificationStatus, SanadProofFilter, SchemeCount,
    SealProofCount, SealProofFilter,
};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

/// Repository for advanced commitment and proof data.
#[derive(Clone)]
pub struct AdvancedProofRepository {
    pool: SqlitePool,
}

impl AdvancedProofRepository {
    /// Create a new advanced proof repository.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Initialize the advanced proof tables.
    pub async fn init(&self) -> Result<(), sqlx::Error> {
        // Enhanced sanads table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS enhanced_sanads (
                id TEXT PRIMARY KEY,
                chain TEXT NOT NULL,
                seal_ref TEXT NOT NULL,
                commitment TEXT NOT NULL,
                owner TEXT NOT NULL,
                created_at DATETIME NOT NULL,
                created_tx TEXT NOT NULL,
                status TEXT NOT NULL,
                metadata TEXT,
                transfer_count BIGINT NOT NULL DEFAULT 0,
                last_transfer_at DATETIME,
                
                -- Advanced fields
                commitment_scheme TEXT NOT NULL DEFAULT 'hash_based',
                commitment_version INTEGER NOT NULL DEFAULT 2,
                protocol_id TEXT,
                mpc_root TEXT,
                domain_separator TEXT,
                
                -- ZK-Proof fields (for future use)
                has_zk_proof BOOLEAN NOT NULL DEFAULT 0,
                zk_proof_system TEXT,
                zk_proof_metadata TEXT,
                zk_proof_verified BOOLEAN,
                zk_proof_data TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Enhanced seals table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS enhanced_seals (
                id TEXT PRIMARY KEY,
                chain TEXT NOT NULL,
                seal_type TEXT NOT NULL,
                seal_ref TEXT NOT NULL,
                sanad_id TEXT,
                status TEXT NOT NULL,
                consumed_at DATETIME,
                consumed_tx TEXT,
                block_height BIGINT NOT NULL,
                
                -- Advanced fields
                seal_proof_type TEXT NOT NULL DEFAULT 'merkle',
                seal_proof_data TEXT,
                seal_proof_verified BOOLEAN,
                seal_proof_metadata TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Enhanced inclusion proofs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS enhanced_inclusion_proofs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sanad_id TEXT NOT NULL,
                chain TEXT NOT NULL,
                anchor_ref TEXT NOT NULL,
                proof_type TEXT NOT NULL,
                proof_data TEXT NOT NULL,
                proof_size_bytes BIGINT NOT NULL,
                verified BOOLEAN,
                proof_metadata TEXT,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Enhanced transfers table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS enhanced_transfers (
                id TEXT PRIMARY KEY,
                sanad_id TEXT NOT NULL,
                from_chain TEXT NOT NULL,
                to_chain TEXT NOT NULL,
                from_owner TEXT NOT NULL,
                to_owner TEXT NOT NULL,
                lock_tx TEXT NOT NULL,
                mint_tx TEXT,
                proof_ref TEXT,
                status TEXT NOT NULL,
                created_at DATETIME NOT NULL,
                completed_at DATETIME,
                duration_ms BIGINT,
                
                -- Advanced fields
                cross_chain_proof_type TEXT,
                cross_chain_proof_data TEXT,
                bridge_contract TEXT,
                bridge_proof_verified BOOLEAN
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Proof statistics cache
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS proof_statistics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chain TEXT NOT NULL,
                commitment_scheme TEXT NOT NULL,
                inclusion_proof_type TEXT NOT NULL,
                finality_proof_type TEXT NOT NULL,
                count BIGINT NOT NULL DEFAULT 1,
                last_updated DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for performance
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_enhanced_sanads_scheme 
            ON enhanced_sanads(commitment_scheme)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_enhanced_sanads_owner 
            ON enhanced_sanads(owner)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_enhanced_seals_proof_type 
            ON enhanced_seals(seal_proof_type)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_inclusion_proofs_sanad 
            ON enhanced_inclusion_proofs(sanad_id)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or update an enhanced sanad record.
    pub async fn insert_enhanced_sanad(
        &self,
        record: &EnhancedSanadRecord,
    ) -> Result<(), sqlx::Error> {
        let metadata_json = record.metadata.as_ref().map(|v| v.to_string());

        sqlx::query(
            r#"
            INSERT INTO enhanced_sanads (
                id, chain, seal_ref, commitment, owner, created_at, created_tx,
                status, metadata, transfer_count, last_transfer_at,
                commitment_scheme, commitment_version, protocol_id, mpc_root, domain_separator,
                inclusion_proof_type, finality_proof_type, proof_size_bytes, confirmations
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
            ON CONFLICT(id) DO UPDATE SET
                status = ?8,
                transfer_count = ?10,
                last_transfer_at = ?11
            "#,
        )
        .bind(&record.id)
        .bind(&record.chain)
        .bind(&record.seal_ref)
        .bind(&record.commitment)
        .bind(&record.owner)
        .bind(record.created_at)
        .bind(&record.created_tx)
        .bind(&record.status)
        .bind(metadata_json)
        .bind(record.transfer_count as i64)
        .bind(record.last_transfer_at)
        .bind(record.commitment_scheme.as_str())
        .bind(record.commitment_version as i64)
        .bind(&record.protocol_id)
        .bind(record.mpc_root.as_deref())
        .bind(record.domain_separator.as_deref())
        .bind(record.inclusion_proof_type.as_str())
        .bind(record.finality_proof_type.as_str())
        .bind(record.proof_size_bytes.map(|v| v as i64))
        .bind(record.confirmations.map(|v| v as i64))
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or update an enhanced seal record.
    pub async fn insert_enhanced_seal(
        &self,
        record: &EnhancedSealRecord,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO enhanced_seals (
                id, chain, seal_type, seal_ref, sanad_id, status, consumed_at,
                consumed_tx, block_height, seal_proof_type, seal_proof_verified
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(id) DO UPDATE SET
                status = ?6,
                seal_proof_verified = ?11
            "#,
        )
        .bind(&record.id)
        .bind(&record.chain)
        .bind(&record.seal_type)
        .bind(&record.seal_ref)
        .bind(record.sanad_id.as_deref())
        .bind(&record.status)
        .bind(record.consumed_at)
        .bind(record.consumed_tx.as_deref())
        .bind(record.block_height as i64)
        .bind(&record.seal_proof_type)
        .bind(record.seal_proof_verified)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert an enhanced inclusion proof.
    pub async fn insert_enhanced_inclusion_proof(
        &self,
        record: &EnhancedInclusionProof,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO enhanced_inclusion_proofs (
                sanad_id, chain, anchor_ref, proof_type, proof_data,
                proof_size_bytes, verified, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&record.sanad_id)
        .bind(&record.chain)
        .bind(&record.anchor_ref)
        .bind(&record.proof_type)
        .bind(&record.proof_data)
        .bind(record.proof_size_bytes as i64)
        .bind(record.verified)
        .bind(record.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert or update an enhanced transfer record.
    pub async fn insert_enhanced_transfer(
        &self,
        record: &EnhancedTransferRecord,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO enhanced_transfers (
                id, sanad_id, from_chain, to_chain, from_owner, to_owner,
                lock_tx, mint_tx, proof_ref, status, created_at, completed_at,
                duration_ms, cross_chain_proof_type, bridge_contract, bridge_proof_verified
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ON CONFLICT(id) DO UPDATE SET
                status = ?10,
                mint_tx = ?8,
                completed_at = ?12,
                duration_ms = ?13,
                bridge_proof_verified = ?16
            "#,
        )
        .bind(&record.id)
        .bind(&record.sanad_id)
        .bind(&record.from_chain)
        .bind(&record.to_chain)
        .bind(&record.from_owner)
        .bind(&record.to_owner)
        .bind(&record.lock_tx)
        .bind(record.mint_tx.as_deref())
        .bind(record.proof_ref.as_deref())
        .bind(&record.status)
        .bind(record.created_at)
        .bind(record.completed_at)
        .bind(record.duration_ms.map(|v| v as i64))
        .bind(record.cross_chain_proof_type.as_deref())
        .bind(record.bridge_contract.as_deref())
        .bind(record.bridge_proof_verified)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Query enhanced sanads with filter.
    pub async fn query_enhanced_sanads(
        &self,
        filter: SanadProofFilter,
    ) -> Result<Vec<EnhancedSanadRecord>, sqlx::Error> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let mut query = String::from("SELECT * FROM enhanced_sanads WHERE 1=1");

        if let Some(ref chain) = filter.chain {
            query.push_str(&format!(" AND chain = '{}'", chain));
        }
        if let Some(ref owner) = filter.owner {
            query.push_str(&format!(" AND owner = '{}'", owner));
        }
        if let Some(scheme) = filter.commitment_scheme {
            query.push_str(&format!(" AND commitment_scheme = '{}'", scheme.as_str()));
        }
        if let Some(proof_type) = filter.inclusion_proof_type {
            query.push_str(&format!(
                " AND inclusion_proof_type = '{}'",
                proof_type.as_str()
            ));
        }

        query.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

        let query_static = Box::leak(query.into_boxed_str()) as &'static str;
        let rows = sqlx::query(query_static).fetch_all(&self.pool).await?;

        let mut records = Vec::new();
        for row in rows {
            records.push(EnhancedSanadRecord {
                id: row.get("id"),
                chain: row.get("chain"),
                seal_ref: row.get("seal_ref"),
                commitment: row.get("commitment"),
                owner: row.get("owner"),
                created_at: row.get("created_at"),
                created_tx: row.get("created_tx"),
                status: row.get("status"),
                metadata: row
                    .try_get::<Option<String>, _>("metadata")
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok()),
                transfer_count: row.get::<i64, _>("transfer_count") as u64,
                last_transfer_at: row.get("last_transfer_at"),
                commitment_scheme: CommitmentScheme::from_str(
                    &row.get::<String, _>("commitment_scheme"),
                )
                .unwrap_or_default(),
                commitment_version: row.get::<i64, _>("commitment_version") as u8,
                protocol_id: row.get("protocol_id"),
                mpc_root: row.get("mpc_root"),
                domain_separator: row.get("domain_separator"),
                inclusion_proof_type: InclusionProofType::from_str(
                    &row.get::<String, _>("inclusion_proof_type"),
                )
                .unwrap_or_default(),
                finality_proof_type: FinalityProofType::from_str(
                    &row.get::<String, _>("finality_proof_type"),
                )
                .unwrap_or_default(),
                proof_size_bytes: row
                    .try_get::<Option<i64>, _>("proof_size_bytes")
                    .ok()
                    .flatten()
                    .map(|v| v as u64),
                confirmations: row
                    .try_get::<Option<i64>, _>("confirmations")
                    .ok()
                    .flatten()
                    .map(|v| v as u64),
            });
        }

        Ok(records)
    }

    /// Query enhanced seals with filter.
    pub async fn query_enhanced_seals(
        &self,
        filter: SealProofFilter,
    ) -> Result<Vec<EnhancedSealRecord>, sqlx::Error> {
        let limit = filter.limit.unwrap_or(20);
        let offset = filter.offset.unwrap_or(0);

        let mut query = String::from("SELECT * FROM enhanced_seals WHERE 1=1");

        if let Some(ref chain) = filter.chain {
            query.push_str(&format!(" AND chain = '{}'", chain));
        }
        if let Some(ref seal_type) = filter.seal_type {
            query.push_str(&format!(" AND seal_type = '{}'", seal_type));
        }
        if let Some(ref proof_type) = filter.seal_proof_type {
            query.push_str(&format!(" AND seal_proof_type = '{}'", proof_type));
        }
        if let Some(verified) = filter.seal_proof_verified {
            query.push_str(&format!(" AND seal_proof_verified = {}", verified));
        }

        query.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

        let query_static = Box::leak(query.into_boxed_str()) as &'static str;
        let rows = sqlx::query(query_static).fetch_all(&self.pool).await?;

        let mut records = Vec::new();
        for row in rows {
            records.push(EnhancedSealRecord {
                id: row.get("id"),
                chain: row.get("chain"),
                seal_type: row.get("seal_type"),
                seal_ref: row.get("seal_ref"),
                sanad_id: row.get("sanad_id"),
                status: row.get("status"),
                consumed_at: row.get("consumed_at"),
                consumed_tx: row.get("consumed_tx"),
                block_height: row.get::<i64, _>("block_height") as u64,
                seal_proof_type: row.get("seal_proof_type"),
                seal_proof_verified: row.get("seal_proof_verified"),
            });
        }

        Ok(records)
    }

    /// Get proof statistics.
    pub async fn get_proof_statistics(&self) -> Result<ProofStatistics, sqlx::Error> {
        // Total sanads
        let total_row = sqlx::query("SELECT COUNT(*) as count FROM enhanced_sanads")
            .fetch_one(&self.pool)
            .await?;
        let total_sanads: i64 = total_row.get("count");

        // Total seals
        let total_seals_row = sqlx::query("SELECT COUNT(*) as count FROM enhanced_seals")
            .fetch_one(&self.pool)
            .await?;
        let total_seals: i64 = total_seals_row.get("count");

        // Sanads by commitment scheme
        let scheme_rows = sqlx::query(
            r#"
            SELECT commitment_scheme, COUNT(*) as count
            FROM enhanced_sanads
            GROUP BY commitment_scheme
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let sanads_by_scheme: Vec<SchemeCount> = scheme_rows
            .into_iter()
            .map(|row| {
                let scheme_str: String = row.get("commitment_scheme");
                let scheme = CommitmentScheme::from_str(&scheme_str).unwrap_or_default();
                let count: i64 = row.get("count");
                SchemeCount {
                    scheme,
                    count: count as u64,
                }
            })
            .collect();

        // Sanads by inclusion proof type
        let incl_rows = sqlx::query(
            r#"
            SELECT inclusion_proof_type, COUNT(*) as count
            FROM enhanced_sanads
            GROUP BY inclusion_proof_type
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let sanads_by_inclusion: Vec<InclusionProofCount> = incl_rows
            .into_iter()
            .map(|row| {
                let proof_str: String = row.get("inclusion_proof_type");
                let proof_type = InclusionProofType::from_str(&proof_str).unwrap_or_default();
                let count: i64 = row.get("count");
                InclusionProofCount {
                    proof_type,
                    count: count as u64,
                }
            })
            .collect();

        // Sanads by finality proof type
        let final_rows = sqlx::query(
            r#"
            SELECT finality_proof_type, COUNT(*) as count
            FROM enhanced_sanads
            GROUP BY finality_proof_type
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let sanads_by_finality: Vec<FinalityProofCount> = final_rows
            .into_iter()
            .map(|row| {
                let proof_str: String = row.get("finality_proof_type");
                let proof_type = FinalityProofType::from_str(&proof_str).unwrap_or_default();
                let count: i64 = row.get("count");
                FinalityProofCount {
                    proof_type,
                    count: count as u64,
                }
            })
            .collect();

        // Seals by proof type
        let seal_rows = sqlx::query(
            r#"
            SELECT seal_proof_type as proof_type, COUNT(*) as count
            FROM enhanced_seals
            GROUP BY seal_proof_type
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let seals_by_type: Vec<SealProofCount> = seal_rows
            .into_iter()
            .map(|row| {
                let proof_type: String = row.get("proof_type");
                let count: i64 = row.get("count");
                SealProofCount {
                    proof_type,
                    count: count as u64,
                }
            })
            .collect();

        Ok(ProofStatistics {
            total_sanads: total_sanads as u64,
            total_seals: total_seals as u64,
            sanads_by_commitment_scheme: sanads_by_scheme,
            sanads_by_inclusion_proof: sanads_by_inclusion,
            sanads_by_finality_proof: sanads_by_finality,
            seals_by_proof_type: seals_by_type,
        })
    }

    /// Update proof verification status for a sanad.
    pub async fn update_sanad_verification_status(
        &self,
        sanad_id: &str,
        status: ProofVerificationStatus,
    ) -> Result<(), sqlx::Error> {
        let verified = match status {
            ProofVerificationStatus::Verified => Some(true),
            ProofVerificationStatus::Invalid => Some(false),
            _ => None,
        };

        sqlx::query(
            r#"
            UPDATE enhanced_sanads
            SET zk_proof_verified = ?1
            WHERE id = ?2
            "#,
        )
        .bind(verified)
        .bind(sanad_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
