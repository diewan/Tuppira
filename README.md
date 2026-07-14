# CSV Explorer

A high-performance multi-chain indexer and explorer UI for CSV (Cross-Chain Sealed Verifiable) sanads, built entirely in Rust with Dioxus for the UI.

## Architecture

```
csv-explorer/
├── shared/      # Shared types, config, and error definitions
├── storage/     # SQLite database layer with repository pattern
├── indexer/     # Multi-chain indexing daemon (Bitcoin, Ethereum, Sui, Aptos, Solana)
├── api/         # GraphQL + REST API for querying indexed data
├── ui/          # Dioxus multiplatform UI (web + desktop)
├── Dockerfile
├── docker-compose.yml
└── config.example.toml
```

### Components

- **Shared** - Core explorer types (`SanadRecord`, `TransferRecord`, `SealRecord`, `CsvContract`), configuration, and error handling
- **Storage** - SQLite database with typed repositories for all entity types, sync progress tracking, and aggregate statistics
- **Indexer** - Chain-agnostic indexing daemon with pluggable `ChainIndexer` trait implementations for Bitcoin, Ethereum, Sui, Aptos, and Solana
- **API** - GraphQL API (primary) + REST API (secondary) with flexible querying, pagination, and filtering
- **UI** - Dioxus fullstack application supporting both web and desktop targets with responsive design, dark mode, and keyboard navigation

## Quick Start

### Prerequisites

- Rust 1.75+
- SQLite3

### Development

```bash
# Clone and build
cargo build --workspace

# Run indexer
cargo run -p csv-explorer-indexer -- start

# Run API server
cargo run -p csv-explorer-api -- start

# Run UI (web)
cargo run -p csv-explorer-ui -- serve

# Run UI (desktop)
cargo run -p csv-explorer-ui -- desktop
```

### Docker

```bash
docker compose up -d
```

## Configuration

Copy `config.example.toml` to `config.toml` and adjust:

```bash
cp config.example.toml config.toml
```

Key configuration sections:

- `[database]` - SQLite connection string
- `[api]` - API server host/port
- `[ui]` - UI server host/port
- `[indexer]` - Concurrency, batch size, poll interval
- `[chains.*]` - Per-chain RPC URLs, networks, start blocks

## API Reference

### GraphQL

Endpoint: `http://localhost:8080/graphql`

Key queries:

```graphql
query GetSanads($filter: SanadFilterInput) {
  sanads(filter: $filter) {
    edges {
      node {
        id
        chain
        owner
        status
        createdAt
      }
    }
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}

query GetStats {
  stats {
    totalSanads
    totalTransfers
    totalSeals
    totalContracts
  }
}
```

### REST

Endpoints:

- `GET /api/v1/sanads` - List sanads with filtering
- `GET /api/v1/sanads/:id` - Get single sanad
- `GET /api/v1/transfers` - List transfers
- `GET /api/v1/seals` - List seals
- `GET /api/v1/stats` - Aggregate statistics
- `GET /api/v1/chains` - Chain status information

## Indexer

The indexer runs as a daemon that continuously polls each enabled chain for CSV-related data.

### Supported Chains

| Chain | Seal Type | Events Tracked |
|-------|-----------|----------------|
| Bitcoin | UTXO/Tapret | OP_RETURN commitments, Tapret proofs |
| Ethereum | Account/Nullifier | Smart contract events, nullifier registry |
| Sui | Object | Object creation/deletion, Move events |
| Aptos | Resource/Nullifier | Resource changes, Move events |
| Solana | Account | Account state changes, transaction logs |

### Commands

```bash
csv-explorer-indexer start          # Start the indexer daemon
csv-explorer-indexer status         # Show current indexer status
csv-explorer-indexer sync <chain>   # Force sync a specific chain
csv-explorer-indexer reindex        # Reindex from a specific block
csv-explorer-indexer reset          # Reset sync progress
```

## Wallet Integration

The UI supports CSV wallet connection for:

- Viewing connected wallet sanads
- Quick transfer initiation
- Balance display

Wallet integration uses the CSV SDK connection protocol.

## Deployment

### Production Docker

```bash
docker compose -f docker-compose.yml up -d
```

### Manual

1. Build release binaries: `cargo build --release --workspace`
2. Configure `config.toml` with production RPC endpoints
3. Start services in order: indexer -> api -> ui

## Metrics

Prometheus metrics are exposed at `/metrics` on the API server:

- `csv_indexer_blocks_indexed_total` - Total blocks indexed per chain
- `csv_indexer_sanads_indexed_total` - Total sanads indexed
- `csv_indexer_transfers_indexed_total` - Total transfers indexed
- `csv_indexer_sync_lag_seconds` - Sync lag per chain
- `csv_indexer_errors_total` - Error counts

## License

MIT OR Apache-2.0
