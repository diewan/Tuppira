#!/bin/bash
# Start Tuppira services

set -e

EXPLORER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$EXPLORER_DIR/.pids"

# Default network
NETWORK="${NETWORK:-testnet}"

# Network-specific configuration
case "$NETWORK" in
    mainnet)
        CONFIG_FILE="config.mainnet.toml"
        API_PORT=8080
        ;;
    testnet)
        CONFIG_FILE="config.testnet.toml"
        API_PORT=8081
        ;;
    *)
        echo "❌ Invalid network: $NETWORK"
        echo "Valid options: mainnet, testnet"
        exit 1
        ;;
esac

export NETWORK API_PORT

# Create PID file if it doesn't exist
touch "$PID_FILE"

start_api() {
    echo "🚀 Starting Tuppira API server ($NETWORK)..."
    if pgrep -f "tuppira-api.*$NETWORK" > /dev/null 2>&1; then
        echo "✅ API server is already running"
    else
        cd "$EXPLORER_DIR"
        # Compile first so the health-check window below times the server's
        # startup, not a cold cargo build (which used to overrun it and fail).
        echo "   building tuppira-api..."
        cargo build -p tuppira-api > "/tmp/tuppira-api-$NETWORK.log" 2>&1
        nohup cargo run -p tuppira-api -- --config "$CONFIG_FILE" start >> "/tmp/tuppira-api-$NETWORK.log" 2>&1 &
        echo $! >> "$PID_FILE"
        sleep 5

        # Health check with retries
        local retries=0
        local max_retries=5
        while [ $retries -lt $max_retries ]; do
            if curl -s "http://localhost:$API_PORT/health" > /dev/null 2>&1; then
                echo "✅ API server started successfully (port $API_PORT)"
                break
            fi
            retries=$((retries + 1))
            sleep 1
        done

        if [ $retries -eq $max_retries ]; then
            echo "❌ API server failed to start. Check logs: /tmp/tuppira-api-$NETWORK.log"
            exit 1
        fi
    fi
}

start_indexer() {
    echo "📡 Starting Tuppira Indexer ($NETWORK)..."
    if pgrep -f "tuppira-indexer.*$NETWORK" > /dev/null 2>&1; then
        echo "✅ Indexer is already running"
    else
        cd "$EXPLORER_DIR"
        echo "   building tuppira-indexer..."
        cargo build -p tuppira-indexer > "/tmp/tuppira-indexer-$NETWORK.log" 2>&1
        nohup cargo run -p tuppira-indexer -- --config "$CONFIG_FILE" start >> "/tmp/tuppira-indexer-$NETWORK.log" 2>&1 &
        echo $! >> "$PID_FILE"
        sleep 3
        echo "✅ Indexer started (see logs: /tmp/tuppira-indexer-$NETWORK.log)"
    fi
}

stop_all() {
    echo "🛑 Stopping all Tuppira services..."
    pkill -f "tuppira-indexer" 2>/dev/null || true
    pkill -f "tuppira-api" 2>/dev/null || true
    rm -f "$PID_FILE"
    echo "✅ All services stopped"
}

status() {
    echo "📊 Tuppira Status ($NETWORK):"
    
    # Check indexer
    if pgrep -f "tuppira-indexer" > /dev/null 2>&1; then
        echo "  ✅ Indexer: Running"
        if [ -f "/tmp/tuppira-indexer-$NETWORK.log" ]; then
            local last_line=$(tail -1 "/tmp/tuppira-indexer-$NETWORK.log" 2>/dev/null)
            echo "     Last log: $last_line"
        fi
    else
        echo "  ❌ Indexer: Not running"
    fi
    
    # Check API
    if curl -s "http://localhost:$API_PORT/health" > /dev/null 2>&1; then
        echo "  ✅ API Server ($API_PORT): Running"
        local stats=$(curl -s "http://localhost:$API_PORT/api/v1/stats" 2>/dev/null)
        if [ -n "$stats" ]; then
            local sanads=$(echo "$stats" | python3 -c "import sys,json; print(json.load(sys.stdin)['data']['total_sanads'])" 2>/dev/null || echo "?")
            local transfers=$(echo "$stats" | python3 -c "import sys,json; print(json.load(sys.stdin)['data']['total_transfers'])" 2>/dev/null || echo "?")
            echo "     Data: $sanads sanads, $transfers transfers"
        fi
    else
        echo "  ❌ API Server ($API_PORT): Not running"
    fi
}

case "${1:-start}" in
    start)
        start_indexer
        start_api
        echo ""
        echo "📊 API: http://localhost:$API_PORT"
        echo "🔍 GraphQL Playground: http://localhost:$API_PORT/playground"
        echo ""
        echo "💡 Monitor indexer logs: tail -f /tmp/tuppira-indexer-$NETWORK.log"
        ;;
    stop)
        stop_all
        ;;
    restart)
        stop_all
        sleep 2
        start_indexer
        start_api
        ;;
    status)
        status
        ;;
    *)
        echo "Usage: $0 {start|stop|restart|status}"
        echo ""
        echo "Network Options:"
        echo "  NETWORK=mainnet ./start.sh  (default)"
        echo "  NETWORK=testnet ./start.sh"
        exit 1
        ;;
esac
