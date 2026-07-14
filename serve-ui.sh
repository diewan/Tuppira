#!/bin/bash
# Serve the CSV Explorer Web UI

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UI_DIR="$SCRIPT_DIR/ui/web"
PORT=${UI_PORT:-3000}

echo "Starting CSV Explorer Web UI on http://localhost:$PORT"
echo "Serving from: $UI_DIR"
echo ""

# Check if Python 3 is available
if command -v python3 &> /dev/null; then
    cd "$UI_DIR"
    python3 -m http.server $PORT
elif command -v python &> /dev/null; then
    cd "$UI_DIR"
    python -m http.server $PORT
else
    echo "Error: Python not found. Please install Python 3 or use another static file server."
    exit 1
fi
