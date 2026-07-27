-- V2 source-closure projections attached to observations (TUP-NE-002).
--
-- Additive only. No existing table, column, trigger, or index is altered, so
-- every Sanad, transfer, seal, and pre-migration observation is byte-identical
-- after this migration and keeps the meaning it was written with. A row here is
-- the closure statement; its absence is not a closure statement of any kind.
CREATE TABLE closure_observations (
    observation_id TEXT PRIMARY KEY REFERENCES observations(observation_id),
    -- The profile that produced the payload, and its own version line. Both are
    -- stored rather than assumed: a reader must be able to reject a payload it
    -- does not understand instead of decoding it under this release's rules.
    profile_id TEXT NOT NULL,
    profile_version INTEGER NOT NULL CHECK (profile_version > 0),
    chain_id TEXT NOT NULL,
    network_id TEXT NOT NULL,
    closure_kind TEXT NOT NULL,
    closure_identity_hex TEXT NOT NULL,
    consumed_transition_id_hex TEXT NOT NULL,
    consumed_output_index INTEGER NOT NULL CHECK (consumed_output_index >= 0),
    successor_commitment_hex TEXT NOT NULL,
    -- NULL when the source disclosed no checkpoint. The reason for its absence
    -- lives in the payload; this column exists only to order and filter.
    observed_checkpoint_height INTEGER CHECK (observed_checkpoint_height IS NULL OR observed_checkpoint_height >= 0),
    indexed_tip_height INTEGER NOT NULL CHECK (indexed_tip_height >= 0),
    -- The exact projection bytes the observation's normalized_payload_digest
    -- commits to. The columns above are query keys derived from these bytes and
    -- are never the authority: a reader that needs a facet reads the payload.
    projection_json TEXT NOT NULL CHECK (length(projection_json) > 0)
);

-- Deliberately not UNIQUE on (chain_id, network_id, closure_kind,
-- closure_identity_hex) or on the consumed state. Two closures competing for
-- one consumed state is equivocation — the exact thing this plane exists to
-- record — and a uniqueness constraint would reject the second report and
-- leave the first looking uncontested.
CREATE INDEX idx_closure_observations_identity
    ON closure_observations(chain_id, network_id, closure_kind, closure_identity_hex);
CREATE INDEX idx_closure_observations_consumed_state
    ON closure_observations(consumed_transition_id_hex, consumed_output_index);
CREATE INDEX idx_closure_observations_successor
    ON closure_observations(successor_commitment_hex);

-- Closure evidence is corrected by appending a superseding observation, never
-- by overwriting the payload an earlier observation committed to.
CREATE TRIGGER closure_observations_no_update BEFORE UPDATE ON closure_observations
BEGIN SELECT RAISE(ABORT, 'closure observations are append-only'); END;
CREATE TRIGGER closure_observations_no_delete BEFORE DELETE ON closure_observations
BEGIN SELECT RAISE(ABORT, 'closure observations are append-only'); END;

-- The payload must belong to the observation that carries it: the observation's
-- declared normalization profile is what a consumer uses to decide how to read
-- these bytes, so a mismatch is a payload of unknown meaning, not a detail.
CREATE TRIGGER closure_observations_match_declared_profile BEFORE INSERT ON closure_observations
WHEN NOT EXISTS (
    SELECT 1 FROM observations o
    WHERE o.observation_id = NEW.observation_id
      AND o.normalized_profile_id = NEW.profile_id
      AND o.normalized_profile_version = NEW.profile_version
)
BEGIN SELECT RAISE(ABORT, 'closure payload does not match the observation profile'); END;
