-- Chain evidence behind a normalized closure observation (TUP-NE-003).
--
-- Additive only. Normalization maps five chain families onto one projection and
-- necessarily drops chain-specific detail — a log index, a spending input, an
-- event sequence number. These tables keep the way back, reached by the same
-- observation identifier the projection is reached by.
CREATE TABLE closure_observation_evidence (
    observation_id TEXT PRIMARY KEY REFERENCES closure_observations(observation_id),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    -- The native event family. Constrained against the projection's closure
    -- kind below: evidence filed under the wrong family would send a reader
    -- back to a chain the closure never happened on.
    native_event_kind TEXT NOT NULL,
    -- The exact source bytes the projection was normalized from.
    raw_event_digest BLOB NOT NULL CHECK (length(raw_event_digest) = 32)
);

-- Locators are ordered rows rather than an encoded list, so a single reference
-- can be looked up directly and the order the connector emitted them survives.
CREATE TABLE closure_observation_evidence_refs (
    observation_id TEXT NOT NULL REFERENCES closure_observation_evidence(observation_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_ref TEXT NOT NULL CHECK (length(evidence_ref) > 0),
    PRIMARY KEY (observation_id, ordinal),
    UNIQUE (observation_id, evidence_ref)
);

CREATE INDEX idx_closure_evidence_refs_ref ON closure_observation_evidence_refs(evidence_ref);

CREATE TRIGGER closure_evidence_no_update BEFORE UPDATE ON closure_observation_evidence
BEGIN SELECT RAISE(ABORT, 'closure chain evidence is append-only'); END;
CREATE TRIGGER closure_evidence_no_delete BEFORE DELETE ON closure_observation_evidence
BEGIN SELECT RAISE(ABORT, 'closure chain evidence is append-only'); END;
CREATE TRIGGER closure_evidence_refs_no_update BEFORE UPDATE ON closure_observation_evidence_refs
BEGIN SELECT RAISE(ABORT, 'closure chain evidence is append-only'); END;
CREATE TRIGGER closure_evidence_refs_no_delete BEFORE DELETE ON closure_observation_evidence_refs
BEGIN SELECT RAISE(ABORT, 'closure chain evidence is append-only'); END;

CREATE TRIGGER closure_evidence_match_closure_kind BEFORE INSERT ON closure_observation_evidence
WHEN NOT EXISTS (
    SELECT 1 FROM closure_observations c
    WHERE c.observation_id = NEW.observation_id
      AND c.closure_kind = NEW.native_event_kind
)
BEGIN SELECT RAISE(ABORT, 'chain evidence does not match the normalized closure family'); END;
