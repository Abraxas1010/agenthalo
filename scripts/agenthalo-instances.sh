#!/usr/bin/env bash
# agenthalo-instances.sh — manage AgentHALO container instances safely
#
# Usage:
#   ./scripts/agenthalo-instances.sh list          Show all instances with roles
#   ./scripts/agenthalo-instances.sh wipe-dev       Destroy ephemeral dev instance + data
#   ./scripts/agenthalo-instances.sh wipe-all       Destroy ALL instances (requires --confirm)
#   ./scripts/agenthalo-instances.sh start-discord   Start Discord bridge
#   ./scripts/agenthalo-instances.sh stop-discord    Stop Discord bridge (data preserved)
#   ./scripts/agenthalo-instances.sh start-dev [--password-mode required|optional|disabled]
#                                                    Start dev/testing instance
#   ./scripts/agenthalo-instances.sh stop-dev        Stop dev/testing instance (data preserved)
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

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

list_instances() {
    echo "=== AgentHALO Instances ==="
    echo ""

    # Docker containers
    local containers
    containers=$(docker ps -a --filter "label=com.agenthalo.role" \
        --format "{{.Names}}\t{{.Status}}\t{{.Label \"com.agenthalo.role\"}}\t{{.Label \"com.agenthalo.persistent\"}}" 2>/dev/null || true)

    if [[ -n "$containers" ]]; then
        printf "%-35s %-20s %-20s %-12s\n" "CONTAINER" "STATUS" "ROLE" "PERSISTENT"
        printf "%-35s %-20s %-20s %-12s\n" "---------" "------" "----" "----------"
        while IFS=$'\t' read -r name status role persistent; do
            local color="$NC"
            if [[ "$persistent" == "true" ]]; then
                color="$GREEN"
            else
                color="$YELLOW"
            fi
            printf "${color}%-35s %-20s %-20s %-12s${NC}\n" "$name" "$status" "$role" "$persistent"
        done <<< "$containers"
    else
        echo "No labeled AgentHALO containers found."
    fi

    echo ""

    # Docker volumes
    local volumes
    volumes=$(docker volume ls --filter "label=com.agenthalo.role" \
        --format "{{.Name}}\t{{.Label \"com.agenthalo.role\"}}\t{{.Label \"com.agenthalo.persistent\"}}" 2>/dev/null || true)

    if [[ -n "$volumes" ]]; then
        printf "%-35s %-20s %-12s\n" "VOLUME" "ROLE" "PERSISTENT"
        printf "%-35s %-20s %-12s\n" "------" "----" "----------"
        while IFS=$'\t' read -r name role persistent; do
            local color="$NC"
            if [[ "$persistent" == "true" ]]; then
                color="$GREEN"
            else
                color="$YELLOW"
            fi
            printf "${color}%-35s %-20s %-12s${NC}\n" "$name" "$role" "$persistent"
        done <<< "$volumes"
    else
        echo "No labeled AgentHALO volumes found."
    fi

    echo ""

    # Host processes
    local procs
    procs=$(pgrep -a agenthalo 2>/dev/null || true)
    if [[ -n "$procs" ]]; then
        echo "Host processes:"
        echo "$procs"
    else
        echo "No host AgentHALO processes."
    fi

    echo ""

    # ~/.agenthalo
    if [[ -d "$HOME/.agenthalo" ]]; then
        echo "Host data: ~/.agenthalo/ ($(du -sh "$HOME/.agenthalo" 2>/dev/null | cut -f1))"
    else
        echo "Host data: ~/.agenthalo/ does not exist"
    fi
}

wipe_dev() {
    echo -e "${YELLOW}Wiping ephemeral dev instance...${NC}"

    # Check for persistent containers that would be caught
    local persistent
    persistent=$(docker ps -a --filter "label=com.agenthalo.persistent=true" \
        --format "{{.Names}}" 2>/dev/null || true)

    docker compose -f docker-compose.yml down -v 2>/dev/null || true

    kill_host_processes

    # Remove host data
    if [[ -d "$HOME/.agenthalo" ]]; then
        rm -rf "$HOME/.agenthalo"
        echo "Removed ~/.agenthalo/"
    fi

    echo -e "${GREEN}Dev instance wiped.${NC}"

    if [[ -n "$persistent" ]]; then
        echo -e "${GREEN}Persistent instances preserved: ${persistent}${NC}"
    fi
}

wipe_all() {
    if [[ "${1:-}" != "--confirm" ]]; then
        echo -e "${RED}This will destroy ALL AgentHALO instances including persistent ones.${NC}"
        echo -e "${RED}Discord recording data will be permanently lost.${NC}"
        echo ""
        echo "Run with --confirm to proceed:"
        echo "  $0 wipe-all --confirm"
        exit 1
    fi

    echo -e "${RED}Wiping ALL instances...${NC}"

    docker compose -f docker-compose.discord.yml down -v 2>/dev/null || true
    docker compose -f docker-compose.yml down -v 2>/dev/null || true

    kill_host_processes

    if [[ -d "$HOME/.agenthalo" ]]; then
        rm -rf "$HOME/.agenthalo"
        echo "Removed ~/.agenthalo/"
    fi

    echo -e "${RED}All instances wiped.${NC}"
}

start_discord() {
    if [[ ! -f deploy/discord.env ]]; then
        echo -e "${RED}deploy/discord.env not found.${NC}"
        echo "Copy the example and configure your token:"
        echo "  cp deploy/discord.env.example deploy/discord.env"
        echo "  # Edit deploy/discord.env — set NUCLEUSDB_DISCORD_TOKEN"
        exit 1
    fi

    echo "Starting Discord bridge..."
    docker compose -f docker-compose.discord.yml up -d --build
    echo ""
    echo -e "${GREEN}Discord bridge running as 'agenthalo-discord-bridge'${NC}"
    echo "Logs: docker logs -f agenthalo-discord-bridge"
}

stop_discord() {
    echo "Stopping Discord bridge (data preserved)..."
    docker compose -f docker-compose.discord.yml down
    echo -e "${GREEN}Discord bridge stopped. Data in volume 'agenthalo-discord-data' preserved.${NC}"
}

start_dev() {
    local password_mode="required"
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
    echo "Starting dev/testing instance..."
    AGENTHALO_PASSWORD_BOOTSTRAP_MODE="$password_mode" docker compose -f docker-compose.yml up -d --build
    echo ""
    echo -e "${GREEN}Dev instance running as 'agenthalo-dev'${NC}"
    echo "Password bootstrap mode: ${password_mode}"
    echo "Dashboard: http://localhost:3100"
    echo "API: http://localhost:8088"
    echo "Logs: docker logs -f agenthalo-dev"
}

stop_dev() {
    echo "Stopping dev instance (data preserved)..."
    docker compose -f docker-compose.yml down
    echo -e "${GREEN}Dev instance stopped. Data in volume 'nucleusdb-dev-data' preserved.${NC}"
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
