-- Source-neutral, append-only observation plane (F-02).
CREATE TABLE retention_classes (
    retention_class_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    purpose TEXT NOT NULL,
    retain_for_seconds INTEGER CHECK (retain_for_seconds IS NULL OR retain_for_seconds >= 0),
    raw_payload_permitted INTEGER NOT NULL CHECK (raw_payload_permitted IN (0, 1))
);

CREATE TABLE observation_sources (
    source_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    connector_kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    retention_class_id TEXT NOT NULL REFERENCES retention_classes(retention_class_id)
);

CREATE TABLE collection_runs (
    collection_run_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    source_id TEXT NOT NULL REFERENCES observation_sources(source_id),
    started_at INTEGER NOT NULL CHECK (started_at > 0),
    completed_at INTEGER CHECK (completed_at IS NULL OR completed_at >= started_at)
);

CREATE TABLE raw_payload_descriptors (
    payload_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    digest_algorithm TEXT NOT NULL,
    payload_digest BLOB NOT NULL CHECK (length(payload_digest) = 32),
    media_type TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    custody_locator TEXT,
    retention_class_id TEXT NOT NULL REFERENCES retention_classes(retention_class_id),
    UNIQUE (digest_algorithm, payload_digest)
);

CREATE TABLE observations (
    observation_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    source_id TEXT NOT NULL REFERENCES observation_sources(source_id),
    source_event_id TEXT NOT NULL,
    source_event_type TEXT NOT NULL,
    asserted_event_time INTEGER CHECK (asserted_event_time IS NULL OR asserted_event_time >= 0),
    observed_at INTEGER NOT NULL CHECK (observed_at > 0),
    normalized_profile_id TEXT NOT NULL,
    normalized_profile_version INTEGER NOT NULL CHECK (normalized_profile_version > 0),
    normalized_payload_digest BLOB NOT NULL CHECK (length(normalized_payload_digest) = 32),
    raw_payload_digest BLOB CHECK (raw_payload_digest IS NULL OR length(raw_payload_digest) = 32),
    collection_run_id TEXT NOT NULL REFERENCES collection_runs(collection_run_id),
    supersedes_observation_id TEXT REFERENCES observations(observation_id),
    retraction_status TEXT NOT NULL CHECK (retraction_status IN ('active', 'retracted')),
    visibility_scope TEXT NOT NULL CHECK (visibility_scope IN ('public', 'tenant')),
    tenant_id TEXT,
    CHECK ((visibility_scope = 'public' AND tenant_id IS NULL) OR
           (visibility_scope = 'tenant' AND tenant_id IS NOT NULL AND length(tenant_id) > 0)),
    CHECK (supersedes_observation_id IS NULL OR supersedes_observation_id <> observation_id),
    UNIQUE (source_id, source_event_id, normalized_payload_digest)
);

CREATE TABLE observation_subjects (
    observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    subject_ref TEXT NOT NULL,
    subject_kind TEXT NOT NULL,
    PRIMARY KEY (observation_id, subject_ref)
);

CREATE TABLE observation_authenticity_refs (
    observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    authenticity_material_ref TEXT NOT NULL,
    PRIMARY KEY (observation_id, authenticity_material_ref)
);

CREATE TABLE observation_edges (
    from_observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    to_observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    relationship TEXT NOT NULL,
    CHECK (from_observation_id <> to_observation_id),
    PRIMARY KEY (from_observation_id, to_observation_id, relationship)
);

CREATE TABLE supersessions (
    superseding_observation_id TEXT PRIMARY KEY REFERENCES observations(observation_id),
    superseded_observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    observed_at INTEGER NOT NULL CHECK (observed_at > 0),
    CHECK (superseding_observation_id <> superseded_observation_id)
);

CREATE TABLE sync_cursors (
    source_id TEXT PRIMARY KEY REFERENCES observation_sources(source_id),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    cursor_version INTEGER NOT NULL CHECK (cursor_version > 0),
    cursor BLOB NOT NULL,
    observed_at INTEGER NOT NULL CHECK (observed_at > 0)
);

CREATE INDEX idx_observations_source_event ON observations(source_id, source_event_id);
CREATE INDEX idx_observations_observed_at ON observations(observed_at);
CREATE INDEX idx_observations_tenant ON observations(tenant_id, observed_at);
CREATE INDEX idx_observation_subjects_subject ON observation_subjects(subject_ref);
CREATE INDEX idx_supersessions_predecessor ON supersessions(superseded_observation_id);
CREATE INDEX idx_collection_runs_source ON collection_runs(source_id, started_at);

-- Observation evidence is corrected by adding lineage, never by overwriting history.
CREATE TRIGGER observations_no_update BEFORE UPDATE ON observations
BEGIN SELECT RAISE(ABORT, 'observations are append-only'); END;
CREATE TRIGGER observations_no_delete BEFORE DELETE ON observations
BEGIN SELECT RAISE(ABORT, 'observations are append-only'); END;
CREATE TRIGGER observations_require_correction_lineage BEFORE INSERT ON observations
WHEN EXISTS (
    SELECT 1 FROM observations prior
    WHERE prior.source_id = NEW.source_id AND prior.source_event_id = NEW.source_event_id
) AND NEW.supersedes_observation_id IS NULL
BEGIN SELECT RAISE(ABORT, 'changed source events require supersession lineage'); END;
CREATE TRIGGER observations_validate_correction_lineage BEFORE INSERT ON observations
WHEN NEW.supersedes_observation_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM observations prior
    WHERE prior.observation_id = NEW.supersedes_observation_id
      AND prior.source_id = NEW.source_id
      AND prior.source_event_id = NEW.source_event_id
)
BEGIN SELECT RAISE(ABORT, 'superseded observation must represent the same source event'); END;
CREATE TRIGGER raw_payload_descriptors_no_update BEFORE UPDATE ON raw_payload_descriptors
BEGIN SELECT RAISE(ABORT, 'raw payload descriptors are append-only'); END;
CREATE TRIGGER raw_payload_descriptors_no_delete BEFORE DELETE ON raw_payload_descriptors
BEGIN SELECT RAISE(ABORT, 'raw payload descriptors are append-only'); END;
CREATE TRIGGER supersessions_no_update BEFORE UPDATE ON supersessions
BEGIN SELECT RAISE(ABORT, 'supersessions are append-only'); END;
CREATE TRIGGER supersessions_no_delete BEFORE DELETE ON supersessions
BEGIN SELECT RAISE(ABORT, 'supersessions are append-only'); END;
