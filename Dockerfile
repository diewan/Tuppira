# syntax=docker/dockerfile:1
# Build from the `Work/` monorepo directory so the protocol path dependencies
# are inside the declared build context:
# docker build -f csv-apps/csv-explorer/Dockerfile --target api .
FROM rust:1.95-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace/csv-apps/csv-explorer

# Both workspaces are deliberate build inputs. No Docker COPY reaches outside
# the context, and the UI is intentionally not part of either production image.
COPY csv-protocol /workspace/csv-protocol
COPY csv-apps/csv-explorer /workspace/csv-apps/csv-explorer

RUN cargo build --locked --release \
    --package csv-explorer-indexer \
    --package csv-explorer-api

FROM debian:bookworm-slim AS runtime-base

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN install -d -o 1000 -g 1000 /app/data
WORKDIR /app
COPY csv-apps/csv-explorer/config.example.toml /app/config.toml
USER 1000

FROM runtime-base AS indexer
COPY --from=builder /workspace/csv-apps/csv-explorer/target/release/csv-explorer-indexer /usr/local/bin/
ENTRYPOINT ["csv-explorer-indexer"]
CMD ["start"]

FROM runtime-base AS api
USER root
RUN apt-get update && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/csv-apps/csv-explorer/target/release/csv-explorer-api /usr/local/bin/
USER 1000
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=10s --retries=3 CMD curl -fsS http://localhost:8080/health || exit 1
ENTRYPOINT ["csv-explorer-api"]
CMD ["start"]
