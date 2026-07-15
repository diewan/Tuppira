#!/bin/bash
# Stop Tuppira services

set -e

EXPLORER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$EXPLORER_DIR/.pids"

echo "🛑 Stopping all Tuppira services..."

# Kill processes by name patterns
echo "  ├─ Stopping API server..."
pkill -f "tuppira-api" 2>/dev/null && echo "  │  ✅ API stopped" || echo "  │  ℹ️  API was not running"

echo "  ├─ Stopping Indexer..."
pkill -f "tuppira-indexer" 2>/dev/null && echo "  │  ✅ Indexer stopped" || echo "  │  ℹ️  Indexer was not running"

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
pkill -f "cargo run -p tuppira" 2>/dev/null && echo "     ✅ Cargo processes stopped" || echo "     ℹ️  No cargo processes found"

echo ""
echo "✅ All services stopped"
