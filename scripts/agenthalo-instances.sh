#!/usr/bin/env bash
# agenthalo-instances.sh — manage native AgentHALO instances safely
#
# Usage:
#   ./scripts/agenthalo-instances.sh list
#   ./scripts/agenthalo-instances.sh wipe-dev
#   ./scripts/agenthalo-instances.sh wipe-all --confirm
#   ./scripts/agenthalo-instances.sh start-discord
#   ./scripts/agenthalo-instances.sh stop-discord
#   ./scripts/agenthalo-instances.sh start-dev [--password-mode required|optional|disabled]
#   ./scripts/agenthalo-instances.sh stop-dev
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_DIR"

RUNTIME_ROOT="${AGENTHALO_RUNTIME_ROOT:-$HOME/.agenthalo-runtimes}"
DEV_ROOT="$RUNTIME_ROOT/dev"
DEV_HOME="$DEV_ROOT/home"
DEV_PID="$DEV_ROOT/agenthalo.pid"
DEV_LOG="$DEV_ROOT/agenthalo.log"

DISCORD_ROOT="$RUNTIME_ROOT/discord"
DISCORD_HOME="$DISCORD_ROOT/home"
DISCORD_PID="$DISCORD_ROOT/nucleusdb-discord.pid"
DISCORD_LOG="$DISCORD_ROOT/nucleusdb-discord.log"
DISCORD_DB_DEFAULT="$DISCORD_ROOT/discord_records.ndb"

NATIVE_SESSION_ROOT="${AGENTHALO_NATIVE_SESSION_ROOT:-${TMPDIR:-/tmp}/agenthalo-native}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

release_bin() {
    printf '%s/target/release/%s' "$REPO_DIR" "$1"
}

pid_running() {
    local pid_file="$1"
    [[ -f "$pid_file" ]] || return 1
    local pid
    pid="$(cat "$pid_file" 2>/dev/null || true)"
    [[ -n "$pid" ]] || return 1
    kill -0 "$pid" 2>/dev/null
}

listener_pid() {
    local port="$1"
    ss -ltnp "( sport = :${port} )" 2>/dev/null \
        | sed -n 's/.*pid=\([0-9]\+\).*/\1/p' \
        | head -n1
}

read_pid() {
    cat "$1" 2>/dev/null || true
}

ensure_binary() {
    local bin_name="$1"
    local bin_path
    bin_path="$(release_bin "$bin_name")"
    if [[ -x "$bin_path" ]]; then
        return 0
    fi
    echo "Building $bin_name..."
    cargo build --release --bin "$bin_name"
}

stop_pid_file() {
    local pid_file="$1"
    local label="$2"
    if ! pid_running "$pid_file"; then
        rm -f "$pid_file"
        return 0
    fi
    local pid
    pid="$(read_pid "$pid_file")"
    echo "Stopping $label (pid $pid)..."
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 40); do
        if ! kill -0 "$pid" 2>/dev/null; then
            rm -f "$pid_file"
            return 0
        fi
        sleep 0.25
    done
    echo "Force stopping $label (pid $pid)..."
    kill -9 "$pid" 2>/dev/null || true
    rm -f "$pid_file"
}

wait_for_http() {
    local url="$1"
    local label="$2"
    for _ in $(seq 1 80); do
        if curl -fsS "$url" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.25
    done
    echo -e "${RED}${label} did not become ready at ${url}${NC}"
    return 1
}

ensure_dev_port_available() {
    local port_pid
    port_pid="$(listener_pid 3100)"
    [[ -z "$port_pid" ]] && return 0
    if [[ -f "/proc/${port_pid}/cmdline" ]] && tr '\0' ' ' <"/proc/${port_pid}/cmdline" | grep -q "agenthalo"; then
        echo -e "${YELLOW}Stopping stale AgentHALO listener on :3100 (pid ${port_pid})...${NC}"
        kill "$port_pid" 2>/dev/null || true
        for _ in $(seq 1 40); do
            if ! kill -0 "$port_pid" 2>/dev/null; then
                return 0
            fi
            sleep 0.25
        done
        echo -e "${RED}Could not stop stale AgentHALO listener on :3100 (pid ${port_pid}).${NC}"
        return 1
    fi
    echo -e "${RED}Port 3100 is already in use by pid ${port_pid}; refusing to start dev dashboard over a non-AgentHALO listener.${NC}"
    return 1
}

wait_for_pid_exit() {
    local pid="$1"
    local label="$2"
    for _ in $(seq 1 40); do
        if ! kill -0 "$pid" 2>/dev/null; then
            return 0
        fi
        sleep 0.25
    done
    echo -e "${RED}${label} did not stop cleanly (pid ${pid}).${NC}"
    return 1
}

start_background() {
    local pid_file="$1"
    local log_file="$2"
    shift 2
    mkdir -p "$(dirname "$pid_file")"
    : > "$log_file"
    (
        cd "$REPO_DIR"
        setsid "$@" >>"$log_file" 2>&1 < /dev/null &
        echo $! > "$pid_file"
    )
}

kill_host_processes() {
    local patterns=(
        "agenthalo-mcp-server"
        "target/debug/agenthalo"
        "target/release/agenthalo"
        "/usr/local/bin/agenthalo"
        "target/debug/nucleusdb"
        "target/release/nucleusdb"
        "/usr/local/bin/nucleusdb"
        "target/debug/nucleusdb-server"
        "target/release/nucleusdb-server"
        "/usr/local/bin/nucleusdb-server"
        "target/debug/nucleusdb-mcp"
        "target/release/nucleusdb-mcp"
        "/usr/local/bin/nucleusdb-mcp"
        "target/debug/nucleusdb-discord"
        "target/release/nucleusdb-discord"
        "/usr/local/bin/nucleusdb-discord"
    )

    local pattern
    for pattern in "${patterns[@]}"; do
        pkill -f "$pattern" 2>/dev/null || true
    done
}

list_instance() {
    local name="$1"
    local pid_file="$2"
    local log_file="$3"
    local home_dir="$4"
    printf "%-12s " "$name"
    if pid_running "$pid_file"; then
        printf "${GREEN}running${NC} "
        printf "(pid %s)" "$(read_pid "$pid_file")"
    else
        printf "${YELLOW}stopped${NC}"
    fi
    echo
    echo "  home: $home_dir"
    echo "  log:  $log_file"
}

list_instances() {
    echo "=== AgentHALO Native Instances ==="
    echo ""
    list_instance "dev" "$DEV_PID" "$DEV_LOG" "$DEV_HOME"
    echo ""
    list_instance "discord" "$DISCORD_PID" "$DISCORD_LOG" "$DISCORD_HOME"
    echo ""
    if [[ -d "$NATIVE_SESSION_ROOT" ]]; then
        echo "Native session dir: $NATIVE_SESSION_ROOT"
        find "$NATIVE_SESSION_ROOT" -mindepth 1 -maxdepth 1 -type d | sed 's#^#  - #' || true
    else
        echo "Native session dir: $NATIVE_SESSION_ROOT (empty)"
    fi
}

wipe_dev() {
    echo -e "${YELLOW}Wiping ephemeral dev instance...${NC}"
    stop_pid_file "$DEV_PID" "dev dashboard"
    local port_pid
    port_pid="$(listener_pid 3100)"
    if [[ -n "$port_pid" ]] && [[ -f "/proc/${port_pid}/cmdline" ]] && tr '\0' ' ' <"/proc/${port_pid}/cmdline" | grep -q "agenthalo"; then
        echo "Stopping stale dev listener on :3100 (pid $port_pid)..."
        kill "$port_pid" 2>/dev/null || true
        wait_for_pid_exit "$port_pid" "stale dev listener"
    fi
    rm -rf "$DEV_ROOT"
    rm -rf "$NATIVE_SESSION_ROOT"
    echo -e "${GREEN}Dev instance wiped.${NC}"
}

wipe_all() {
    if [[ "${1:-}" != "--confirm" ]]; then
        echo -e "${RED}This will destroy ALL local AgentHALO instances and data.${NC}"
        echo "Run with --confirm to proceed:"
        echo "  $0 wipe-all --confirm"
        exit 1
    fi
    echo -e "${RED}Wiping ALL instances...${NC}"
    stop_pid_file "$DISCORD_PID" "discord bridge"
    stop_pid_file "$DEV_PID" "dev dashboard"
    kill_host_processes
    rm -rf "$DEV_ROOT" "$DISCORD_ROOT" "$NATIVE_SESSION_ROOT"
    echo -e "${GREEN}All instances wiped.${NC}"
}

start_dev() {
    local password_mode="disabled"
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --password-mode)
                password_mode="${2:-}"
                shift 2
                ;;
            *)
                echo -e "${RED}Unknown start-dev option: $1${NC}"
                echo "Usage: $0 start-dev [--password-mode required|optional|disabled]"
                exit 1
                ;;
        esac
    done
    case "$password_mode" in
        required|optional|disabled) ;;
        *)
            echo -e "${RED}Invalid password mode: ${password_mode}${NC}"
            echo "Expected one of: required, optional, disabled"
            exit 1
            ;;
    esac

    if pid_running "$DEV_PID"; then
        echo -e "${YELLOW}Dev dashboard already running (pid $(read_pid "$DEV_PID")).${NC}"
        echo "Dashboard: http://localhost:3100"
        return 0
    fi

    ensure_binary agenthalo
    ensure_dev_port_available
    mkdir -p "$DEV_ROOT" "$DEV_HOME"
    echo "Starting dev/testing instance..."
    start_background \
        "$DEV_PID" \
        "$DEV_LOG" \
        env \
        AGENTHALO_HOME="$DEV_HOME" \
        AGENTHALO_DASHBOARD_BOOTSTRAP_MODE="$password_mode" \
        "$(release_bin agenthalo)" \
        dashboard \
        --port 3100 \
        --no-open
    wait_for_http "http://127.0.0.1:3100/api/status" "Dev dashboard"
    local dev_pid listener
    dev_pid="$(read_pid "$DEV_PID")"
    listener="$(listener_pid 3100)"
    if [[ -z "$listener" || "$listener" != "$dev_pid" ]]; then
        echo -e "${RED}Dev dashboard readiness check resolved to pid ${listener:-none}, expected ${dev_pid:-none}.${NC}"
        echo -e "${RED}Refusing to treat a stale listener as a successful start.${NC}"
        return 1
    fi
    echo ""
    echo -e "${GREEN}Dev instance running natively.${NC}"
    echo "Password bootstrap mode: ${password_mode}"
    echo "Dashboard: http://localhost:3100"
    echo "Logs: $DEV_LOG"
}

stop_dev() {
    stop_pid_file "$DEV_PID" "dev dashboard"
    echo -e "${GREEN}Dev instance stopped.${NC}"
}

start_discord() {
    if [[ ! -f deploy/discord.env ]]; then
        echo -e "${RED}deploy/discord.env not found.${NC}"
        echo "Copy the example and configure your token:"
        echo "  cp deploy/discord.env.example deploy/discord.env"
        exit 1
    fi
    if pid_running "$DISCORD_PID"; then
        echo -e "${YELLOW}Discord bridge already running (pid $(read_pid "$DISCORD_PID")).${NC}"
        return 0
    fi

    ensure_binary nucleusdb-discord
    mkdir -p "$DISCORD_ROOT" "$DISCORD_HOME"
    set -a
    # shellcheck disable=SC1091
    source deploy/discord.env
    set +a
    export AGENTHALO_HOME="$DISCORD_HOME"
    export NUCLEUSDB_DISCORD_DB_PATH="${NUCLEUSDB_DISCORD_DB_PATH:-$DISCORD_DB_DEFAULT}"

    echo "Starting Discord bridge..."
    start_background "$DISCORD_PID" "$DISCORD_LOG" "$(release_bin nucleusdb-discord)"
    sleep 2
    if ! pid_running "$DISCORD_PID"; then
        echo -e "${RED}Discord bridge exited immediately. Check $DISCORD_LOG${NC}"
        return 1
    fi
    echo -e "${GREEN}Discord bridge running natively.${NC}"
    echo "Logs: $DISCORD_LOG"
}

stop_discord() {
    stop_pid_file "$DISCORD_PID" "discord bridge"
    echo -e "${GREEN}Discord bridge stopped.${NC}"
}

case "${1:-}" in
    list)          list_instances ;;
    wipe-dev)      wipe_dev ;;
    wipe-all)      wipe_all "${2:-}" ;;
    start-discord) start_discord ;;
    stop-discord)  stop_discord ;;
    start-dev)     shift; start_dev "$@" ;;
    stop-dev)      stop_dev ;;
    *)
        echo "Usage: $0 {list|wipe-dev|wipe-all|start-discord|stop-discord|start-dev|stop-dev}"
        exit 1
        ;;
esac
