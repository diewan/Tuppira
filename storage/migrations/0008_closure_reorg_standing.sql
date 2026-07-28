-- Reorganization standing for V2 closure observations (TUP-NE-004).
--
-- Additive only. No existing table, column, trigger, or index is altered. A
-- reorganization is recorded here as a statement placed *beside* the closure
-- observation it names, never as an edit to it: `closure_observations` is
-- append-only by trigger and stays that way, so an orphaned closure is still
-- readable exactly as the source first reported it. Deleting it would destroy
-- the only record that the source once said it.

CREATE TABLE closure_observation_orphanings (
    reorg_id TEXT NOT NULL REFERENCES source_reorgs(reorg_id),
    observation_id TEXT NOT NULL REFERENCES closure_observations(observation_id),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    orphaned_at INTEGER NOT NULL CHECK (orphaned_at > 0),
    -- 'superseded' is the source reporting this closure again on the
    -- replacement history; 'retracted' is the source reporting that it did not
    -- reappear. They are different statements and the replacement column below
    -- is what keeps them from being written as if they were the same one.
    disposition TEXT NOT NULL CHECK (disposition IN ('superseded', 'retracted')),
    superseding_observation_id TEXT REFERENCES closure_observations(observation_id),
    PRIMARY KEY (reorg_id, observation_id),
    CHECK (superseding_observation_id IS NULL OR superseding_observation_id <> observation_id),
    CHECK ((disposition = 'superseded' AND superseding_observation_id IS NOT NULL)
        OR (disposition = 'retracted' AND superseding_observation_id IS NULL))
);

-- Why the source reported no replacement. Ordered rows rather than an encoded
-- list, so a reason survives as the connector emitted it. Only a retraction has
-- reasons; the trigger below rejects reasons filed against a supersession,
-- which would read as a withdrawal the source never made.
CREATE TABLE closure_observation_orphaning_reasons (
    reorg_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    reason TEXT NOT NULL CHECK (length(reason) > 0),
    PRIMARY KEY (reorg_id, observation_id, ordinal),
    UNIQUE (reorg_id, observation_id, reason),
    FOREIGN KEY (reorg_id, observation_id)
        REFERENCES closure_observation_orphanings(reorg_id, observation_id)
);

CREATE INDEX idx_closure_orphanings_observation
    ON closure_observation_orphanings(observation_id);

-- How far the index has reached on each chain and network.
--
-- Derived index state, not evidence: no observation commits to it and nothing
-- here is digest-bound. It exists so a closure read can report the lag it is
-- being read under, rather than only the lag the collector recorded when the
-- observation was first normalized. A view separated from its current lag
-- reads as current, which is the failure this table prevents.
CREATE TABLE closure_index_tips (
    chain_id TEXT NOT NULL,
    network_id TEXT NOT NULL,
    indexed_tip_height INTEGER NOT NULL CHECK (indexed_tip_height >= 0),
    indexed_tip_block_id_hex TEXT NOT NULL CHECK (length(indexed_tip_block_id_hex) > 0),
    indexed_tip_observed_at INTEGER NOT NULL CHECK (indexed_tip_observed_at > 0),
    PRIMARY KEY (chain_id, network_id)
);

-- Seed from the freshness readings already committed inside stored closure
-- projections, so the first read after this migration is not measured against
-- an empty index. Advancing the tip is the writer's job from here on.
INSERT INTO closure_index_tips (
    chain_id, network_id, indexed_tip_height, indexed_tip_block_id_hex, indexed_tip_observed_at
)
SELECT
    c.chain_id,
    c.network_id,
    c.indexed_tip_height,
    json_extract(c.projection_json, '$.index_freshness.indexed_tip_block_id_hex'),
    json_extract(c.projection_json, '$.index_freshness.indexed_tip_observed_at')
FROM closure_observations c
JOIN (
    SELECT chain_id, network_id, MAX(indexed_tip_height) AS tip
    FROM closure_observations GROUP BY chain_id, network_id
) best
  ON best.chain_id = c.chain_id
 AND best.network_id = c.network_id
 AND best.tip = c.indexed_tip_height
WHERE json_extract(c.projection_json, '$.index_freshness.indexed_tip_block_id_hex') IS NOT NULL
  AND json_extract(c.projection_json, '$.index_freshness.indexed_tip_observed_at') > 0
GROUP BY c.chain_id, c.network_id;

-- A reorganization is a discontinuity in one source's history. Letting it
-- orphan another source's observation would let one connector withdraw
-- evidence it never collected.
CREATE TRIGGER closure_orphanings_match_reorg_source BEFORE INSERT ON closure_observation_orphanings
WHEN NOT EXISTS (
    SELECT 1 FROM source_reorgs r
    JOIN observations o ON o.observation_id = NEW.observation_id
    WHERE r.reorg_id = NEW.reorg_id AND r.source_id = o.source_id
)
BEGIN SELECT RAISE(ABORT, 'a reorganization orphans only observations of its own source'); END;

-- An orphaning recorded before its observation was collected describes an
-- order of events that did not happen.
CREATE TRIGGER closure_orphanings_follow_observation BEFORE INSERT ON closure_observation_orphanings
WHEN NOT EXISTS (
    SELECT 1 FROM observations o
    WHERE o.observation_id = NEW.observation_id AND o.observed_at <= NEW.orphaned_at
)
BEGIN SELECT RAISE(ABORT, 'an orphaning cannot precede the observation it names'); END;

-- A replacement is the same closure re-reported on the replacement history, so
-- it closes the same consumed state. One that closes a different state is a
-- different closure, and recording it as a replacement would silently move the
-- subject of the source's statement.
CREATE TRIGGER closure_orphanings_replace_same_consumed_state BEFORE INSERT ON closure_observation_orphanings
WHEN NEW.superseding_observation_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM closure_observations orphaned
    JOIN closure_observations replacement
      ON replacement.observation_id = NEW.superseding_observation_id
    WHERE orphaned.observation_id = NEW.observation_id
      AND orphaned.chain_id = replacement.chain_id
      AND orphaned.network_id = replacement.network_id
      AND orphaned.consumed_transition_id_hex = replacement.consumed_transition_id_hex
      AND orphaned.consumed_output_index = replacement.consumed_output_index
)
BEGIN SELECT RAISE(ABORT, 'a replacement closure must close the same consumed state'); END;

CREATE TRIGGER closure_orphaning_reasons_require_retraction BEFORE INSERT ON closure_observation_orphaning_reasons
WHEN NOT EXISTS (
    SELECT 1 FROM closure_observation_orphanings o
    WHERE o.reorg_id = NEW.reorg_id
      AND o.observation_id = NEW.observation_id
      AND o.disposition = 'retracted'
)
BEGIN SELECT RAISE(ABORT, 'only a retracted orphaning carries reasons'); END;

-- Reorganization history is corrected by recording the next reorganization,
-- never by rewriting the last one.
CREATE TRIGGER closure_orphanings_no_update BEFORE UPDATE ON closure_observation_orphanings
BEGIN SELECT RAISE(ABORT, 'closure orphanings are append-only'); END;
CREATE TRIGGER closure_orphanings_no_delete BEFORE DELETE ON closure_observation_orphanings
BEGIN SELECT RAISE(ABORT, 'closure orphanings are append-only'); END;
CREATE TRIGGER closure_orphaning_reasons_no_update BEFORE UPDATE ON closure_observation_orphaning_reasons
BEGIN SELECT RAISE(ABORT, 'closure orphanings are append-only'); END;
CREATE TRIGGER closure_orphaning_reasons_no_delete BEFORE DELETE ON closure_observation_orphaning_reasons
BEGIN SELECT RAISE(ABORT, 'closure orphanings are append-only'); END;

-- The tip only ever advances. A tip that could move backwards would let a lag
-- shrink without the index having caught up.
CREATE TRIGGER closure_index_tips_advance_only BEFORE UPDATE ON closure_index_tips
WHEN NEW.indexed_tip_height < OLD.indexed_tip_height
BEGIN SELECT RAISE(ABORT, 'the indexed tip never moves backwards'); END;
CREATE TRIGGER closure_index_tips_no_delete BEFORE DELETE ON closure_index_tips
BEGIN SELECT RAISE(ABORT, 'indexed tips are not deleted'); END;
