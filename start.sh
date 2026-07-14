#!/bin/bash
# Start CSV Explorer services

set -e

EXPLORER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$EXPLORER_DIR/.pids"

# Default network
NETWORK="${NETWORK:-testnet}"

# Network-specific configuration
case "$NETWORK" in
    mainnet)
        CONFIG_FILE="config.mainnet.toml"
        UI_PORT=3000
        API_PORT=8080
        ;;
    testnet)
        CONFIG_FILE="config.testnet.toml"
        UI_PORT=3001
        API_PORT=8081
        ;;
    *)
        echo "❌ Invalid network: $NETWORK"
        echo "Valid options: mainnet, testnet"
        exit 1
        ;;
esac

export NETWORK UI_PORT API_PORT

# Create PID file if it doesn't exist
touch "$PID_FILE"

start_api() {
    echo "🚀 Starting CSV Explorer API server ($NETWORK)..."
    if pgrep -f "csv-explorer-api.*$NETWORK" > /dev/null 2>&1; then
        echo "✅ API server is already running"
    else
        cd "$EXPLORER_DIR"
        nohup cargo run -p csv-explorer-api -- --config "$CONFIG_FILE" start > "/tmp/csv-explorer-api-$NETWORK.log" 2>&1 &
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
            echo "❌ API server failed to start. Check logs: /tmp/csv-explorer-api-$NETWORK.log"
            exit 1
        fi
    fi
}

start_indexer() {
    echo "📡 Starting CSV Explorer Indexer ($NETWORK)..."
    if pgrep -f "csv-explorer-indexer.*$NETWORK" > /dev/null 2>&1; then
        echo "✅ Indexer is already running"
    else
        cd "$EXPLORER_DIR"
        nohup cargo run -p csv-explorer-indexer -- --config "$CONFIG_FILE" start > "/tmp/csv-explorer-indexer-$NETWORK.log" 2>&1 &
        echo $! >> "$PID_FILE"
        sleep 3
        echo "✅ Indexer started (see logs: /tmp/csv-explorer-indexer-$NETWORK.log)"
    fi
}

start_ui() {
    echo "🎨 Starting CSV Explorer UI ($NETWORK)..."
    if pgrep -f "http.server.*$UI_PORT" > /dev/null 2>&1; then
        echo "✅ UI server is already running"
    else
        cd "$EXPLORER_DIR/ui/web"
        nohup python3 -m http.server $UI_PORT > "/tmp/csv-explorer-ui-$NETWORK.log" 2>&1 &
        echo $! >> "$PID_FILE"
        sleep 2
        
        # Health check
        if curl -s "http://localhost:$UI_PORT" > /dev/null 2>&1; then
            echo "✅ UI server started (port $UI_PORT)"
        else
            echo "❌ UI server failed to start. Check logs: /tmp/csv-explorer-ui-$NETWORK.log"
            exit 1
        fi
    fi
}

stop_all() {
    echo "🛑 Stopping all CSV Explorer services..."
    pkill -f "csv-explorer-indexer" 2>/dev/null || true
    pkill -f "csv-explorer-api" 2>/dev/null || true
    pkill -f "http.server.*300" 2>/dev/null || true
    pkill -f "dx serve" 2>/dev/null || true
    rm -f "$PID_FILE"
    echo "✅ All services stopped"
}

status() {
    echo "📊 CSV Explorer Status ($NETWORK):"
    
    # Check indexer
    if pgrep -f "csv-explorer-indexer" > /dev/null 2>&1; then
        echo "  ✅ Indexer: Running"
        if [ -f "/tmp/csv-explorer-indexer-$NETWORK.log" ]; then
            local last_line=$(tail -1 "/tmp/csv-explorer-indexer-$NETWORK.log" 2>/dev/null)
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

    # Check UI
    if curl -s "http://localhost:$UI_PORT" > /dev/null 2>&1; then
        echo "  ✅ UI Server ($UI_PORT): Running"
    else
        echo "  ❌ UI Server ($UI_PORT): Not running"
    fi
}

case "${1:-start}" in
    start)
        start_indexer
        start_api
        start_ui
        echo ""
        echo "🌐 Explorer UI: http://localhost:$UI_PORT"
        echo "📊 API: http://localhost:$API_PORT"
        echo "🔍 GraphQL Playground: http://localhost:$API_PORT/playground"
        echo ""
        echo "💡 Monitor indexer logs: tail -f /tmp/csv-explorer-indexer-$NETWORK.log"
        ;;
    stop)
        stop_all
        ;;
    restart)
        stop_all
        sleep 2
        start_indexer
        start_api
        start_ui
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
