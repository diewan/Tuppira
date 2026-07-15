-- Durable, ordered explorer observations for reconnecting wallet clients.
CREATE TABLE wallet_feed_events (
    sequence INTEGER PRIMARY KEY,
    observation_id TEXT NOT NULL UNIQUE,
    envelope_json TEXT NOT NULL,
    indexed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_wallet_feed_events_observation_id ON wallet_feed_events(observation_id);
