#!/usr/bin/env bash
set -euo pipefail

NYM_ID="${NYM_ID:-agenthalo}"
NYM_PORT="${NYM_PORT:-1080}"
NYM_DATA_DIR="${NYM_DATA_DIR:-/data/nym}"
WDK_PORT="${WDK_PORT:-7321}"
DASHBOARD_PORT="${AGENTHALO_DASHBOARD_PORT:-3100}"
MCP_PORT="${AGENTHALO_MCP_PORT:-8390}"
NUCLEUSDB_PORT="${NUCLEUSDB_PORT:-8088}"
LOG_DIR="${AGENTHALO_HOME:-/data}/logs"
NOMIC_MODEL_DIR="${NOMIC_MODEL_DIR:-/opt/models/nomic-embed-text}"

export AGENTHALO_DASHBOARD_HOST="${AGENTHALO_DASHBOARD_HOST:-0.0.0.0}"
export AGENTHALO_MCP_HOST="${AGENTHALO_MCP_HOST:-0.0.0.0}"

if [[ -z "${WDK_AUTH_TOKEN:-}" ]]; then
  WDK_AUTH_TOKEN="$(openssl rand -hex 32)"
fi
export WDK_AUTH_TOKEN
export WDK_PORT

if [[ -z "${AGENTHALO_MCP_SECRET:-}" ]]; then
  AGENTHALO_MCP_SECRET="$(openssl rand -hex 32)"
  export AGENTHALO_MCP_SECRET
  echo "[AgentHALO] $(date -u +%Y-%m-%dT%H:%M:%SZ) Generated ephemeral AGENTHALO_MCP_SECRET for this container boot."
fi

log() { echo "[AgentHALO] $(date -u +%Y-%m-%dT%H:%M:%SZ) $*"; }
die() { log "FATAL: $*" >&2; exit 1; }

wait_for_port() {
  local name="$1"
  local port="$2"
  local timeout="${3:-30}"
  local deadline=$((SECONDS + timeout))
  while ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1; do
    if [[ "$SECONDS" -ge "$deadline" ]]; then
      die "${name} did not become ready on port ${port} within ${timeout}s"
    fi
    sleep 0.5
  done
}

wait_for_http() {
  local name="$1"
  local url="$2"
  local timeout="${3:-30}"
  local deadline=$((SECONDS + timeout))
  while ! curl -sf "$url" >/dev/null 2>&1; do
    if [[ "$SECONDS" -ge "$deadline" ]]; then
      die "${name} did not become healthy at ${url} within ${timeout}s"
    fi
    sleep 0.5
  done
}

declare -A PIDS
SHUTTING_DOWN=0

start_bg() {
  local name="$1"
  local logfile="$2"
  shift 2
  log "Starting ${name}..."
  "$@" >"${logfile}" 2>&1 &
  local pid=$!
  PIDS["$name"]="$pid"
}

stop_all() {
  if [[ "$SHUTTING_DOWN" -eq 1 ]]; then
    return
  fi
  SHUTTING_DOWN=1
  log "Shutting down child processes..."
  local order=(dashboard mcp nucleusdb wdk nym)
  local name
  for name in "${order[@]}"; do
    local pid="${PIDS[$name]:-}"
    if [[ -n "$pid" ]] && kill -0 "$pid" >/dev/null 2>&1; then
      kill "$pid" >/dev/null 2>&1 || true
    fi
  done
  wait || true
  log "All child processes stopped."
}

on_signal() {
  log "Received termination signal."
  stop_all
  exit 0
}

trap on_signal SIGTERM SIGINT
trap stop_all EXIT

mkdir -p "$LOG_DIR" "$NYM_DATA_DIR"
chmod 700 "${AGENTHALO_HOME:-/data}" || true

if [[ ! -f "${NOMIC_MODEL_DIR}/model.onnx" ]]; then
  die "nomic-embed-text model not found at ${NOMIC_MODEL_DIR}/model.onnx"
fi
if [[ ! -f "${NOMIC_MODEL_DIR}/tokenizer.json" ]]; then
  die "nomic-embed-text tokenizer not found at ${NOMIC_MODEL_DIR}/tokenizer.json"
fi

log "============================================"
log " Agent H.A.L.O. Unified Container Starting"
log "============================================"
log " AGENTHALO_HOME: ${AGENTHALO_HOME:-/data}"
log " NOMIC_MODEL_DIR: ${NOMIC_MODEL_DIR}"
log " NYM_FAIL_OPEN:  ${NYM_FAIL_OPEN:-false}"
log " SOCKS5_PROXY:   ${SOCKS5_PROXY:-not set}"
log " Dashboard:      ${DASHBOARD_PORT}"
log " MCP:            ${MCP_PORT}"
log "============================================"

if [[ -n "${NYM_PROVIDER:-}" ]]; then
  NYM_CONFIG_DIR="${NYM_DATA_DIR}/socks5-clients/${NYM_ID}"
  if [[ ! -d "$NYM_CONFIG_DIR" ]]; then
    log "Initializing Nym SOCKS5 identity: ${NYM_ID}"
    nym-socks5-client init \
      --id "${NYM_ID}" \
      --provider "${NYM_PROVIDER}" \
      --port "${NYM_PORT}" \
      2>&1 | tee "${LOG_DIR}/nym_init.log"
  else
    log "Reusing existing Nym identity at ${NYM_CONFIG_DIR}"
  fi
  start_bg "nym" "${LOG_DIR}/nym.log" nym-socks5-client run --id "${NYM_ID}"
  wait_for_port "Nym SOCKS5" "${NYM_PORT}" 60
  log "Nym mixnet transport active."
else
  log "WARNING: NYM_PROVIDER not set; Nym transport disabled."
  log "External requests requiring mixnet routing will be blocked when fail-closed."
fi

if [[ -f /opt/wdk-sidecar/index.mjs ]]; then
  start_bg "wdk" "${LOG_DIR}/wdk.log" node /opt/wdk-sidecar/index.mjs
  wait_for_port "WDK sidecar" "${WDK_PORT}" 10
else
  log "WDK sidecar assets missing; wallet features unavailable."
fi

start_bg "nucleusdb" "${LOG_DIR}/nucleusdb.log" nucleusdb-server "0.0.0.0:${NUCLEUSDB_PORT}" production
wait_for_port "NucleusDB" "${NUCLEUSDB_PORT}" 15

start_bg "mcp" "${LOG_DIR}/mcp.log" agenthalo-mcp-server
wait_for_http "MCP server" "http://127.0.0.1:${MCP_PORT}/health" 20

start_bg "dashboard" "${LOG_DIR}/dashboard.log" agenthalo dashboard --port "${DASHBOARD_PORT}" --no-open
wait_for_http "Dashboard" "http://127.0.0.1:${DASHBOARD_PORT}/api/status" 30

log "============================================"
log " All services ready."
log " Dashboard:  http://localhost:${DASHBOARD_PORT}"
log " MCP:        http://localhost:${MCP_PORT}"
log " NucleusDB:  127.0.0.1:${NUCLEUSDB_PORT} (internal)"
log " WDK:        127.0.0.1:${WDK_PORT} (internal)"
log " Nym:        127.0.0.1:${NYM_PORT} (internal)"
log "============================================"

while true; do
  EXITED_PID=""
  if wait -n -p EXITED_PID; then
    EXIT_CODE=0
  else
    EXIT_CODE=$?
  fi

  EXITED_NAME="unknown"
  for NAME in "${!PIDS[@]}"; do
    if [[ "${PIDS[$NAME]}" == "$EXITED_PID" ]]; then
      EXITED_NAME="$NAME"
      break
    fi
  done

  log "Process exited: ${EXITED_NAME} (pid=${EXITED_PID}, code=${EXIT_CODE})"
  stop_all
  exit "$EXIT_CODE"
done
