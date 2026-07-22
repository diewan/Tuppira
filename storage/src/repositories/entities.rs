//! Tenant-scoped accountable-entity read projections.

use sqlx::{Row, SqlitePool};
use tuppira_shared::{Result, TuppiraError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityProfile {
    pub entity_id: String,
    pub entity_kind: String,
    pub display_name: String,
    pub profile_digest_hex: String,
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountabilityReference {
    pub ref_kind: String,
    pub disclosure_state: String,
    pub object_id: Option<String>,
    pub source_observation_id: Option<String>,
    pub observed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRelationship {
    pub relationship_kind: String,
    pub disclosure_state: String,
    pub related_entity_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityAggregate {
    pub profile: EntityProfile,
    pub references: Vec<AccountabilityReference>,
    pub relationships: Vec<EntityRelationship>,
}

#[derive(Clone)]
pub struct EntityRepository {
    pool: SqlitePool,
}

impl EntityRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn profile(&self, tenant_id: &str, entity_id: &str) -> Result<EntityProfile> {
        ensure_scope(tenant_id, entity_id)?;
        let row = sqlx::query("SELECT entity_kind, display_name, profile_digest_hex, updated_at FROM accountable_entities WHERE tenant_id = ? AND entity_id = ?")
            .bind(tenant_id).bind(entity_id).fetch_optional(&self.pool).await?
            .ok_or_else(|| TuppiraError::NotFound { entity_type: "accountable_entity".into(), id: entity_id.into() })?;
        Ok(EntityProfile {
            entity_id: entity_id.into(),
            entity_kind: row.try_get("entity_kind")?,
            display_name: row.try_get("display_name")?,
            profile_digest_hex: row.try_get("profile_digest_hex")?,
            updated_at: u64::try_from(row.try_get::<i64, _>("updated_at")?)
                .map_err(|_| invalid())?,
        })
    }

    pub async fn aggregate(&self, tenant_id: &str, entity_id: &str) -> Result<EntityAggregate> {
        let profile = self.profile(tenant_id, entity_id).await?;
        let refs = sqlx::query("SELECT ref_kind, disclosure_state, object_id, source_observation_id, observed_at FROM entity_accountability_refs WHERE tenant_id = ? AND entity_id = ? ORDER BY observed_at, ref_kind")
            .bind(tenant_id).bind(entity_id).fetch_all(&self.pool).await?;
        let relationships = sqlx::query("SELECT relationship_kind, disclosure_state, related_entity_id FROM entity_relationships WHERE tenant_id = ? AND entity_id = ? ORDER BY relationship_kind, related_entity_id")
            .bind(tenant_id).bind(entity_id).fetch_all(&self.pool).await?;
        Ok(EntityAggregate {
            profile,
            references: refs
                .into_iter()
                .map(|row| {
                    Ok(AccountabilityReference {
                        ref_kind: row.try_get("ref_kind")?,
                        disclosure_state: row.try_get("disclosure_state")?,
                        object_id: row.try_get("object_id")?,
                        source_observation_id: row.try_get("source_observation_id")?,
                        observed_at: u64::try_from(row.try_get::<i64, _>("observed_at")?)
                            .map_err(|_| invalid())?,
                    })
                })
                .collect::<Result<_>>()?,
            relationships: relationships
                .into_iter()
                .map(|row| {
                    Ok(EntityRelationship {
                        relationship_kind: row.try_get("relationship_kind")?,
                        disclosure_state: row.try_get("disclosure_state")?,
                        related_entity_id: row.try_get("related_entity_id")?,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}

fn ensure_scope(tenant_id: &str, entity_id: &str) -> Result<()> {
    if tenant_id.trim().is_empty() || entity_id.trim().is_empty() {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> TuppiraError {
    TuppiraError::Parse("entity scope or projection value is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool;

    #[tokio::test]
    async fn aggregate_is_tenant_scoped_and_preserves_withheld_relationships() {
        let pool = init_pool("sqlite::memory:", 1).await.unwrap();
        for tenant in ["a", "b"] {
            sqlx::query(
                "INSERT INTO accountable_entities VALUES (?, 'entity-1', 'agent', ?, ?, 1)",
            )
            .bind(tenant)
            .bind(tenant)
            .bind("11".repeat(32))
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO retention_classes VALUES ('retention',1,'test',NULL,0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO observation_sources VALUES ('source',1,'test','Test','retention')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO collection_runs VALUES ('run',1,'source',1,1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO observations VALUES ('observation-1',1,'source','event-1','mandate',NULL,1,'profile',1,?,NULL,'run',NULL,'active','tenant','a')")
            .bind(vec![1_u8; 32]).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO entity_accountability_refs VALUES ('a','entity-1','mandate','available','mandate-1','observation-1',2)").execute(&pool).await.unwrap();
        let cross_tenant = sqlx::query("INSERT INTO entity_accountability_refs VALUES ('b','entity-1','mandate','available','mandate-1','observation-1',2)").execute(&pool).await;
        assert!(cross_tenant.is_err());
        sqlx::query("INSERT INTO entity_relationships VALUES ('a','entity-1','delegated_by','withheld',NULL)").execute(&pool).await.unwrap();
        let repo = EntityRepository::new(pool);
        let a = repo.aggregate("a", "entity-1").await.unwrap();
        assert_eq!(a.profile.display_name, "a");
        assert_eq!(a.references.len(), 1);
        assert_eq!(a.relationships[0].disclosure_state, "withheld");
        assert_eq!(a.relationships[0].related_entity_id, None);
        let b = repo.aggregate("b", "entity-1").await.unwrap();
        assert_eq!(b.profile.display_name, "b");
        assert!(b.references.is_empty());
    }
}
