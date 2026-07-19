-- Append-only reconciliation history (F-07).
CREATE TABLE source_reorgs (
    reorg_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    source_id TEXT NOT NULL REFERENCES observation_sources(source_id),
    detected_at INTEGER NOT NULL CHECK (detected_at > 0),
    prior_tip TEXT NOT NULL,
    replacement_tip TEXT NOT NULL,
    CHECK (length(prior_tip) > 0 AND length(replacement_tip) > 0),
    CHECK (prior_tip <> replacement_tip)
);

CREATE TABLE contradiction_hints (
    hint_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    left_observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    right_observation_id TEXT NOT NULL REFERENCES observations(observation_id),
    detector_id TEXT NOT NULL,
    detected_at INTEGER NOT NULL CHECK (detected_at > 0),
    CHECK (left_observation_id <> right_observation_id),
    UNIQUE (left_observation_id, right_observation_id, detector_id)
);

CREATE INDEX idx_source_reorgs_source_time ON source_reorgs(source_id, detected_at);
CREATE INDEX idx_contradiction_hints_left ON contradiction_hints(left_observation_id);
CREATE INDEX idx_contradiction_hints_right ON contradiction_hints(right_observation_id);

CREATE TRIGGER source_reorgs_no_update BEFORE UPDATE ON source_reorgs
BEGIN SELECT RAISE(ABORT, 'source reorgs are append-only'); END;
CREATE TRIGGER source_reorgs_no_delete BEFORE DELETE ON source_reorgs
BEGIN SELECT RAISE(ABORT, 'source reorgs are append-only'); END;
CREATE TRIGGER contradiction_hints_no_update BEFORE UPDATE ON contradiction_hints
BEGIN SELECT RAISE(ABORT, 'contradiction hints are append-only'); END;
CREATE TRIGGER contradiction_hints_no_delete BEFORE DELETE ON contradiction_hints
BEGIN SELECT RAISE(ABORT, 'contradiction hints are append-only'); END;

-- A disagreement is meaningful only between different sources, about at least
-- one common subject, and within one disclosure boundary.
CREATE TRIGGER contradiction_hints_validate BEFORE INSERT ON contradiction_hints
WHEN NOT EXISTS (
    SELECT 1 FROM observations l JOIN observations r
      ON l.observation_id = NEW.left_observation_id
     AND r.observation_id = NEW.right_observation_id
    WHERE l.source_id <> r.source_id
      AND l.visibility_scope = r.visibility_scope
      AND COALESCE(l.tenant_id, '') = COALESCE(r.tenant_id, '')
      AND EXISTS (
          SELECT 1 FROM observation_subjects ls
          JOIN observation_subjects rs ON rs.subject_ref = ls.subject_ref
          WHERE ls.observation_id = l.observation_id
            AND rs.observation_id = r.observation_id
      )
)
BEGIN SELECT RAISE(ABORT, 'contradiction must be cross-source, same-subject, and same-visibility'); END;
