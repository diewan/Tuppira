# syntax=docker/dockerfile:1

###############################################################################
# Base builder stage
###############################################################################
FROM rust:1.95-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    sqlite3 \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace Cargo.toml first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY shared/Cargo.toml shared/
COPY storage/Cargo.toml storage/
COPY indexer/Cargo.toml indexer/
COPY api/Cargo.toml api/
COPY ui/Cargo.toml ui/

# Create dummy source files for dependency resolution
RUN mkdir -p shared/src storage/src indexer/src api/src ui/src && \
    echo "pub fn dummy() {}" > shared/src/lib.rs && \
    echo "pub fn dummy() {}" > storage/src/lib.rs && \
    echo "pub fn dummy() {}" > indexer/src/lib.rs && \
    echo "pub fn dummy() {}" > indexer/src/main.rs && \
    echo "pub fn dummy() {}" > api/src/lib.rs && \
    echo "pub fn dummy() {}" > api/src/main.rs && \
    echo "fn main() {}" > ui/src/main.rs

# Build dependencies only (caching layer)
RUN cargo build --workspace --release && rm -rf /app/target/release/deps/*

# Copy actual source
COPY shared/src/ shared/src/
COPY storage/src/ storage/src/
COPY indexer/src/ indexer/src/
COPY api/src/ api/src/
COPY ui/src/ ui/src/

# Build everything
RUN cargo build --workspace --release

###############################################################################
# Indexer runtime
###############################################################################
FROM debian:bookworm-slim AS indexer

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    sqlite3 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /app/data && chown -R 1000:1000 /app
WORKDIR /app

COPY --from=builder /app/target/release/csv-explorer-indexer /usr/local/bin/
COPY config.example.toml /app/config.toml

USER 1000
EXPOSE 9090

ENTRYPOINT ["csv-explorer-indexer"]
CMD ["start"]

###############################################################################
# API runtime
###############################################################################
FROM debian:bookworm-slim AS api

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    sqlite3 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /app/data && chown -R 1000:1000 /app
WORKDIR /app

COPY --from=builder /app/target/release/csv-explorer-api /usr/local/bin/
COPY config.example.toml /app/config.toml

USER 1000
EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=10s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["csv-explorer-api"]
CMD ["start"]

###############################################################################
# UI runtime (web)
###############################################################################
FROM debian:bookworm-slim AS ui

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/csv-explorer-ui /usr/local/bin/
COPY config.example.toml /app/config.toml

EXPOSE 3000

ENTRYPOINT ["csv-explorer-ui"]
CMD ["serve"]
