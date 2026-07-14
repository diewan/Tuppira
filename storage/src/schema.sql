-- CSV Explorer database schema
-- Apply with: sqlite3 explorer.db < schema.sql

-- Sanads table
CREATE TABLE IF NOT EXISTS sanads (
    id TEXT PRIMARY KEY,
    chain TEXT NOT NULL,
    seal_ref TEXT NOT NULL,
    commitment TEXT NOT NULL,
    owner TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    created_tx TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    metadata TEXT,
    transfer_count INTEGER DEFAULT 0,
    last_transfer_at TIMESTAMP,
    indexed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Transfers table
CREATE TABLE IF NOT EXISTS transfers (
    id TEXT PRIMARY KEY,
    sanad_id TEXT NOT NULL REFERENCES sanads(id),
    from_chain TEXT NOT NULL,
    to_chain TEXT NOT NULL,
    from_owner TEXT NOT NULL,
    to_owner TEXT NOT NULL,
    lock_tx TEXT NOT NULL,
    mint_tx TEXT,
    proof_ref TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL,
    completed_at TIMESTAMP,
    duration_ms INTEGER,
    indexed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Seals table
CREATE TABLE IF NOT EXISTS seals (
    id TEXT PRIMARY KEY,
    chain TEXT NOT NULL,
    seal_type TEXT NOT NULL,
    seal_ref TEXT NOT NULL,
    sanad_id TEXT REFERENCES sanads(id),
    status TEXT NOT NULL DEFAULT 'available',
    consumed_at TIMESTAMP,
    consumed_tx TEXT,
    block_height BIGINT NOT NULL,
    indexed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Contracts table
CREATE TABLE IF NOT EXISTS contracts (
    id TEXT PRIMARY KEY,
    chain TEXT NOT NULL,
    contract_type TEXT NOT NULL,
    address TEXT NOT NULL,
    deployed_tx TEXT NOT NULL,
    deployed_at TIMESTAMP NOT NULL,
    version TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    indexed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Sync progress table
CREATE TABLE IF NOT EXISTS sync_progress (
    chain TEXT PRIMARY KEY,
    latest_block BIGINT NOT NULL DEFAULT 0,
    latest_slot BIGINT,
    last_synced_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_sanads_chain ON sanads(chain);
CREATE INDEX IF NOT EXISTS idx_sanads_owner ON sanads(owner);
CREATE INDEX IF NOT EXISTS idx_sanads_status ON sanads(status);
CREATE INDEX IF NOT EXISTS idx_transfers_sanad_id ON transfers(sanad_id);
CREATE INDEX IF NOT EXISTS idx_transfers_status ON transfers(status);
CREATE INDEX IF NOT EXISTS idx_seals_chain ON seals(chain);
CREATE INDEX IF NOT EXISTS idx_seals_status ON seals(status);
CREATE INDEX IF NOT EXISTS idx_seals_sanad_id ON seals(sanad_id);
CREATE INDEX IF NOT EXISTS idx_seals_seal_ref ON seals(seal_ref);
CREATE INDEX IF NOT EXISTS idx_sanads_seal_ref ON sanads(seal_ref);
CREATE INDEX IF NOT EXISTS idx_contracts_chain ON contracts(chain);
CREATE INDEX IF NOT EXISTS idx_contracts_status ON contracts(status);
