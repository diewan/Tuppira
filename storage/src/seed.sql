-- Seed data for CSV Explorer testing
-- This script populates the database with sample data for testing

-- Insert sample sanads
INSERT OR IGNORE INTO sanads (id, chain, seal_ref, commitment, owner, created_at, created_tx, status, metadata, transfer_count, last_transfer_at) VALUES
('sanad_btc_001', 'bitcoin', 'utxo_txid_abc123', '0x1a2b3c4d5e6f7890abcdef1234567890', 'bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh', '2024-01-15 10:30:00', 'tx_btc_001', 'active', '{"type": "digital_asset", "name": "Bitcoin Sanad #1"}', 2, '2024-01-20 14:15:00'),
('sanad_btc_002', 'bitcoin', 'utxo_txid_def456', '0x2b3c4d5e6f7890abcdef1234567890ab', 'bc1q9h5yjq3gk2v7h8f9d0s1a2z3x4c5v6b7n8m9', '2024-01-16 11:45:00', 'tx_btc_002', 'active', '{"type": "token", "name": "Bitcoin Sanad #2"}', 0, NULL),
('sanad_eth_001', 'ethereum', 'account_0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb', '0x3c4d5e6f7890abcdef1234567890abcd', '0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb', '2024-01-17 09:20:00', 'tx_eth_001', 'spent', '{"type": "nft", "name": "Ethereum Sanad #1"}', 3, '2024-01-25 16:30:00'),
('sanad_eth_002', 'ethereum', 'account_0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed', '0x4d5e6f7890abcdef1234567890abcdef', '0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed', '2024-01-18 13:10:00', 'tx_eth_002', 'active', '{"type": "certificate", "name": "Ethereum Sanad #2"}', 1, '2024-01-22 10:45:00'),
('sanad_sui_001', 'sui', 'object_0x1234567890abcdef', '0x5e6f7890abcdef1234567890abcdef12', '0x1234567890abcdef1234567890abcdef', '2024-01-19 15:30:00', 'tx_sui_001', 'active', '{"type": "gaming_asset", "name": "Sui Sanad #1"}', 0, NULL),
('sanad_sui_002', 'sui', 'object_0xabcdef1234567890', '0x6f7890abcdef1234567890abcdef1234', '0xabcdef1234567890abcdef1234567890', '2024-01-20 08:00:00', 'tx_sui_002', 'pending', '{"type": "identity", "name": "Sui Sanad #2"}', 0, NULL),
('sanad_aptos_001', 'aptos', 'resource_0x1::CSV::Sanad', '0x7890abcdef1234567890abcdef123456', '0x1234567890abcdef', '2024-01-21 12:15:00', 'tx_aptos_001', 'active', '{"type": "credential", "name": "Aptos Sanad #1"}', 1, '2024-01-23 09:30:00'),
('sanad_solana_001', 'solana', 'pda_CsvSanad123', '0x890abcdef1234567890abcdef1234567', 'CsvPDA1234567890abcdef1234567890', '2024-01-22 14:45:00', 'tx_sol_001', 'active', '{"type": "membership", "name": "Solana Sanad #1"}', 0, NULL);

-- Insert sample transfers
INSERT OR IGNORE INTO transfers (id, sanad_id, from_chain, to_chain, from_owner, to_owner, lock_tx, mint_tx, proof_ref, status, created_at, completed_at, duration_ms) VALUES
('xfer_001', 'sanad_btc_001', 'bitcoin', 'ethereum', 'bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh', '0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb', 'tx_lock_001', 'tx_mint_001', 'proof_001', 'completed', '2024-01-20 10:00:00', '2024-01-20 14:15:00', 15300000),
('xfer_002', 'sanad_btc_001', 'ethereum', 'sui', '0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb', '0x1234567890abcdef1234567890abcdef', 'tx_lock_002', NULL, NULL, 'in_progress', '2024-01-25 08:30:00', NULL, NULL),
('xfer_003', 'sanad_eth_001', 'ethereum', 'aptos', '0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb', '0x1234567890abcdef', 'tx_lock_003', 'tx_mint_003', 'proof_003', 'completed', '2024-01-18 11:00:00', '2024-01-18 15:45:00', 17100000),
('xfer_004', 'sanad_eth_001', 'aptos', 'solana', '0x1234567890abcdef', 'CsvPDA1234567890abcdef1234567890', 'tx_lock_004', 'tx_mint_004', 'proof_004', 'completed', '2024-01-20 09:00:00', '2024-01-20 12:30:00', 12600000),
('xfer_005', 'sanad_eth_001', 'solana', 'bitcoin', 'CsvPDA1234567890abcdef1234567890', 'bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh', 'tx_lock_005', NULL, NULL, 'pending', '2024-01-25 16:30:00', NULL, NULL),
('xfer_006', 'sanad_eth_002', 'ethereum', 'sui', '0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed', '0x1234567890abcdef1234567890abcdef', 'tx_lock_006', NULL, NULL, 'failed', '2024-01-22 07:00:00', '2024-01-22 10:45:00', 13500000),
('xfer_007', 'sanad_aptos_001', 'aptos', 'ethereum', '0x1234567890abcdef', '0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed', 'tx_lock_007', 'tx_mint_007', 'proof_007', 'completed', '2024-01-23 06:00:00', '2024-01-23 09:30:00', 12600000);

-- Insert sample seals
INSERT OR IGNORE INTO seals (id, chain, seal_type, seal_ref, sanad_id, status, consumed_at, consumed_tx, block_height) VALUES
('seal_btc_001', 'bitcoin', 'utxo', 'utxo_seal_abc123', 'sanad_btc_001', 'consumed', '2024-01-20 10:00:00', 'tx_consume_001', 840123),
('seal_btc_002', 'bitcoin', 'utxo', 'utxo_seal_def456', 'sanad_btc_002', 'available', NULL, NULL, 840456),
('seal_btc_003', 'bitcoin', 'tapret', 'tapret_seal_789', NULL, 'available', NULL, NULL, 840789),
('seal_eth_001', 'ethereum', 'nullifier', 'nullifier_0x123', 'sanad_eth_001', 'consumed', '2024-01-18 11:00:00', 'tx_consume_002', 19500123),
('seal_eth_002', 'ethereum', 'account', 'account_0x456', 'sanad_eth_002', 'available', NULL, NULL, 19500456),
('seal_sui_001', 'sui', 'object', 'object_0x789', 'sanad_sui_001', 'available', NULL, NULL, 120000789),
('seal_sui_002', 'sui', 'object', 'object_0xabc', 'sanad_sui_002', 'available', NULL, NULL, 120000123),
('seal_aptos_001', 'aptos', 'resource', 'resource_0x1::CSV', 'sanad_aptos_001', 'consumed', '2024-01-23 06:00:00', 'tx_consume_003', 95000123),
('seal_aptos_002', 'aptos', 'nullifier', 'nullifier_aptos_001', NULL, 'available', NULL, NULL, 95000456),
('seal_sol_001', 'solana', 'account', 'pda_SeaL123', 'sanad_solana_001', 'available', NULL, NULL, 250000123),
('seal_sol_002', 'solana', 'account', 'pda_SeaL456', NULL, 'available', NULL, NULL, 250000456);

-- Insert sample contracts
INSERT OR IGNORE INTO contracts (id, chain, contract_type, address, deployed_tx, deployed_at, version, status) VALUES
('contract_btc_nullifier', 'bitcoin', 'nullifier_registry', 'bc1qCSVNullifierRegistry', 'tx_deploy_btc_001', '2023-12-01 10:00:00', '1.0.0', 'active'),
('contract_btc_registry', 'bitcoin', 'sanad_registry', 'bc1qCSVSanadRegistry', 'tx_deploy_btc_002', '2023-12-01 10:30:00', '1.0.0', 'active'),
('contract_eth_nullifier', 'ethereum', 'nullifier_registry', '0x1234567890abcdef1234567890abcdef12345678', 'tx_deploy_eth_001', '2023-12-05 14:00:00', '1.2.0', 'active'),
('contract_eth_registry', 'ethereum', 'sanad_registry', '0xabcdef1234567890abcdef1234567890abcdef12', 'tx_deploy_eth_002', '2023-12-05 14:30:00', '1.2.0', 'active'),
('contract_eth_bridge', 'ethereum', 'bridge', '0x5678901234abcdef5678901234abcdef56789012', 'tx_deploy_eth_003', '2023-12-10 09:00:00', '1.1.0', 'active'),
('contract_sui_registry', 'sui', 'sanad_registry', '0x123::CSVRegistry::Registry', 'tx_deploy_sui_001', '2023-12-15 11:00:00', '1.0.0', 'active'),
('contract_sui_bridge', 'sui', 'bridge', '0x456::CSVBridge::Bridge', 'tx_deploy_sui_002', '2023-12-15 11:30:00', '1.0.0', 'deprecated'),
('contract_aptos_registry', 'aptos', 'sanad_registry', '0x1::CSVRegistry', 'tx_deploy_aptos_001', '2023-12-20 13:00:00', '1.0.0', 'active'),
('contract_aptos_nullifier', 'aptos', 'nullifier_registry', '0x1::CSVNullifier', 'tx_deploy_aptos_002', '2023-12-20 13:30:00', '1.0.0', 'active'),
('contract_sol_registry', 'solana', 'sanad_registry', 'CsvRegistry111111111111111111111111111', 'tx_deploy_sol_001', '2023-12-25 15:00:00', '1.0.0', 'active'),
('contract_sol_bridge', 'solana', 'bridge', 'CsvBridge1111111111111111111111111111', 'tx_deploy_sol_002', '2023-12-25 15:30:00', '1.0.0', 'error');

-- Insert sync progress
INSERT OR IGNORE INTO sync_progress (chain, latest_block, latest_slot, last_synced_at) VALUES
('bitcoin', 840500, NULL, '2024-01-25 18:00:00'),
('ethereum', 19500500, NULL, '2024-01-25 18:00:00'),
('sui', 120000500, NULL, '2024-01-25 17:55:00'),
('aptos', 95000500, NULL, '2024-01-25 18:00:00'),
('solana', 250000500, NULL, '2024-01-25 18:00:00');
