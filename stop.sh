#!/bin/bash
# Stop CSV Explorer services

set -e

EXPLORER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$EXPLORER_DIR/.pids"

echo "🛑 Stopping all CSV Explorer services..."

# Kill processes by name patterns
echo "  ├─ Stopping UI server..."
pkill -f "python3 -m http.server 3000" 2>/dev/null && echo "  │  ✅ UI stopped" || echo "  │  ℹ️  UI was not running"

echo "  ├─ Stopping API server..."
pkill -f "csv-explorer-api" 2>/dev/null && echo "  │  ✅ API stopped" || echo "  │  ℹ️  API was not running"

echo "  ├─ Stopping Indexer..."
pkill -f "csv-explorer-indexer" 2>/dev/null && echo "  │  ✅ Indexer stopped" || echo "  │  ℹ️  Indexer was not running"

# Kill any processes tracked by PID file
if [ -f "$PID_FILE" ]; then
    echo "  ├─ Killing tracked processes..."
    while read -r pid; do
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null && echo "  │  ✅ Process $pid stopped"
        fi
    done < "$PID_FILE"
    rm -f "$PID_FILE"
fi

# Clean up any remaining cargo processes for this project
echo "  └─ Cleaning up cargo dev processes..."
pkill -f "cargo run -p csv-explorer" 2>/dev/null && echo "     ✅ Cargo processes stopped" || echo "     ℹ️  No cargo processes found"

echo ""
echo "✅ All services stopped"
