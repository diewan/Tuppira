# syntax=docker/dockerfile:1
#
# Tuppira observation-plane images (indexer + api).
#
# Updated for the diewan monorepo layout (DEP-04). Build context is the diewan
# root so tuppira's path dependency on ../parwana resolves, and the DEP-01
# shared builder base (diewan/rust-base) supplies the pre-compiled parwana
# dependency graph via the shared CARGO_TARGET_DIR=/workspace/target.
#
#   docker build -f tuppira/Dockerfile --target api     -t diewan/tuppira-api:0.1     .
#   docker build -f tuppira/Dockerfile --target indexer -t diewan/tuppira-indexer:0.1 .
#
# Compose builds it with context ../.. from deployment/compose/.
ARG RUST_BASE=diewan/rust-base:latest

# ── builder ──────────────────────────────────────────────────────────────────
FROM ${RUST_BASE} AS builder
# parwana must sit at /workspace/parwana so tuppira's ../parwana path dep resolves.
COPY parwana /workspace/parwana
COPY tuppira /workspace/tuppira
WORKDIR /workspace/tuppira
RUN cargo build --locked --release \
    --package tuppira-indexer \
    --package tuppira-api

# ── runtime base ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime-base
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-0 curl \
    && rm -rf /var/lib/apt/lists/*
RUN install -d -o 1000 -g 1000 /app/data
WORKDIR /app
# Ship the config profiles; the running service selects one with --config.
COPY tuppira/config.example.toml tuppira/config.testnet.toml tuppira/config.mainnet.toml /app/
USER 1000

# ── indexer (also runs the one-shot `ingest-piteka`) ─────────────────────────
FROM runtime-base AS indexer
# Output lives under the shared CARGO_TARGET_DIR, not the workspace-local target.
COPY --from=builder /workspace/target/release/tuppira-indexer /usr/local/bin/
ENTRYPOINT ["tuppira-indexer"]
CMD ["--config", "config.testnet.toml", "start"]

# ── api (read model, port 8081 under the testnet profile) ────────────────────
FROM runtime-base AS api
COPY --from=builder /workspace/target/release/tuppira-api /usr/local/bin/
EXPOSE 8081
HEALTHCHECK --interval=30s --timeout=10s --retries=5 --start-period=15s \
    CMD curl -fsS http://localhost:8081/health || exit 1
ENTRYPOINT ["tuppira-api"]
CMD ["--config", "config.testnet.toml", "start"]
