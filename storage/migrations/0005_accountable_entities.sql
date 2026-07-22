-- Tenant-scoped accountable-entity read projections (ENT-01).
CREATE TABLE accountable_entities (
    tenant_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    entity_kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    profile_digest_hex TEXT NOT NULL CHECK (length(profile_digest_hex) = 64),
    updated_at INTEGER NOT NULL CHECK (updated_at > 0),
    PRIMARY KEY (tenant_id, entity_id)
);

CREATE TABLE entity_accountability_refs (
    tenant_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    ref_kind TEXT NOT NULL CHECK (ref_kind IN ('mandate', 'receipt', 'dispute', 'anchor')),
    disclosure_state TEXT NOT NULL CHECK (disclosure_state IN ('available', 'incomplete', 'withheld')),
    object_id TEXT,
    source_observation_id TEXT REFERENCES observations(observation_id),
    observed_at INTEGER NOT NULL CHECK (observed_at > 0),
    FOREIGN KEY (tenant_id, entity_id) REFERENCES accountable_entities(tenant_id, entity_id),
    CHECK ((disclosure_state = 'available' AND object_id IS NOT NULL AND source_observation_id IS NOT NULL) OR
           (disclosure_state <> 'available' AND object_id IS NULL))
);

CREATE TABLE entity_relationships (
    tenant_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    relationship_kind TEXT NOT NULL,
    disclosure_state TEXT NOT NULL CHECK (disclosure_state IN ('available', 'incomplete', 'withheld')),
    related_entity_id TEXT,
    FOREIGN KEY (tenant_id, entity_id) REFERENCES accountable_entities(tenant_id, entity_id),
    CHECK ((disclosure_state = 'available' AND related_entity_id IS NOT NULL) OR
           (disclosure_state <> 'available' AND related_entity_id IS NULL))
);

CREATE INDEX entity_refs_scope_idx ON entity_accountability_refs(tenant_id, entity_id, ref_kind, observed_at);
CREATE INDEX entity_relationships_scope_idx ON entity_relationships(tenant_id, entity_id);

CREATE TRIGGER entity_refs_require_visible_source BEFORE INSERT ON entity_accountability_refs
WHEN NEW.source_observation_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM observations source
    WHERE source.observation_id = NEW.source_observation_id
      AND (source.visibility_scope = 'public' OR source.tenant_id = NEW.tenant_id)
)
BEGIN SELECT RAISE(ABORT, 'accountability reference source is not tenant-visible'); END;
