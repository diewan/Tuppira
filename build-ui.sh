#!/bin/bash
# Build and serve CSV Explorer Web UI

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PORT=${UI_PORT:-3000}
API_URL=${API_URL:-http://localhost:8080}

echo "==================================="
echo "  CSV Explorer Web UI Builder"
echo "==================================="
echo ""
echo "Port: $PORT"
echo "API URL: $API_URL"
echo ""

# Check if dx (dioxus-cli) is available
if command -v dx &> /dev/null; then
    echo "Using dioxus-cli (dx)..."
    echo ""
    echo "Starting dev server..."
    dx serve --platform web --addr 0.0.0.0 --port $PORT
else
    echo "dioxus-cli (dx) not found."
    echo ""
    echo "Install it with:"
    echo "  cargo install dioxus-cli"
    echo ""
    echo "Or use the pre-built binary:"
    echo "  ./target/release/csv-explorer-ui serve"
    echo ""
    echo "Note: The pre-built binary requires WASM assets to be built first."
    echo ""
    echo "To build WASM and serve:"
    echo "  1. Install dioxus-cli: cargo install dioxus-cli"
    echo "  2. Build WASM: dx build --platform web --release"
    echo "  3. Serve the built assets with any static file server"
fi
