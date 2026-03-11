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
EMBEDDINGS_REQUIRED="${AGENTHALO_REQUIRE_EMBEDDING_MODEL:-0}"

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

ensure_self_mesh_connect() {
  local enabled="${AGENTHALO_ATTACH_SELF_TO_MESH:-1}"
  case "${enabled,,}" in
    0|false|no|off)
      return 0
      ;;
  esac
  if ! command -v docker >/dev/null 2>&1; then
    log "WARN: docker CLI unavailable; skipping self mesh attach"
    return 0
  fi
  if [[ ! -S /var/run/docker.sock ]]; then
    log "WARN: docker socket unavailable; skipping self mesh attach"
    return 0
  fi
  local self_name="${NUCLEUSDB_SELF_CONTAINER_NAME:-${HOSTNAME:-}}"
  if [[ -z "${self_name}" ]]; then
    self_name="$(hostname 2>/dev/null || true)"
  fi
  if [[ -z "${self_name}" ]]; then
    log "WARN: unable to resolve self container name; skipping self mesh attach"
    return 0
  fi
  if ! docker network inspect halo-mesh >/dev/null 2>&1; then
    docker network create --driver bridge --label nucleusdb.mesh=true halo-mesh >/dev/null 2>&1 || true
  fi
  if docker inspect --format '{{json .NetworkSettings.Networks}}' "${self_name}" 2>/dev/null | grep -q '"halo-mesh"'; then
    return 0
  fi
  if docker network connect halo-mesh "${self_name}" >/dev/null 2>&1; then
    log "Attached ${self_name} to halo-mesh for container RPC"
  else
    log "WARN: failed to attach ${self_name} to halo-mesh"
  fi
}

wait_for_port() {
  local name="$1"
  local port="$2"
  local timeout="${3:-30}"
  local fatal="${4:-true}"
  local deadline=$((SECONDS + timeout))
  while ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1; do
    if [[ "$SECONDS" -ge "$deadline" ]]; then
      if [[ "$fatal" == "true" ]]; then
        die "${name} did not become ready on port ${port} within ${timeout}s"
      else
        log "WARN: ${name} did not become ready on port ${port} within ${timeout}s"
        return 1
      fi
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

if [[ ! -f "${NOMIC_MODEL_DIR}/model.onnx" ]] || [[ ! -f "${NOMIC_MODEL_DIR}/tokenizer.json" ]]; then
  if [[ "${EMBEDDINGS_REQUIRED}" == "1" ]] || [[ "${EMBEDDINGS_REQUIRED}" == "true" ]]; then
    die "nomic-embed-text files missing in ${NOMIC_MODEL_DIR} (set AGENTHALO_REQUIRE_EMBEDDING_MODEL=0 to boot without local embeddings)"
  fi
  log "WARN: nomic-embed-text files missing in ${NOMIC_MODEL_DIR}; semantic memory embeddings will be unavailable until the model is provisioned."
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

ensure_self_mesh_connect

# -- Nym mixnet transport (must start before any service that makes outbound requests) --
# The proxy env vars (HTTP_PROXY, SOCKS5_PROXY, etc.) are baked into the image.
# All outbound traffic routes through Nym. If Nym cannot start, outbound fails
# (fail-closed) unless NYM_FAIL_OPEN=true.
NYM_STARTED=0
NYM_CONFIG_DIR="${NYM_DATA_DIR}/socks5-clients/${NYM_ID}"
NYM_MAX_PROVIDER_ATTEMPTS="${NYM_MAX_PROVIDER_ATTEMPTS:-3}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DISCOVER_SCRIPT="${SCRIPT_DIR}/nym-discover-provider.sh"

try_nym_provider() {
  local provider="$1"
  local attempt_label="$2"
  log "Trying Nym provider (${attempt_label}): ${provider:0:40}..."

  # Clean previous failed init so we can retry with a different provider
  if [[ -d "$NYM_CONFIG_DIR" ]]; then
    rm -rf "$NYM_CONFIG_DIR"
  fi

  if nym-socks5-client init \
      --id "${NYM_ID}" \
      --provider "${provider}" \
      --port "${NYM_PORT}" \
      >>"${LOG_DIR}/nym_init.log" 2>&1; then
    start_bg "nym" "${LOG_DIR}/nym.log" nym-socks5-client run --id "${NYM_ID}"
    if wait_for_port "Nym SOCKS5" "${NYM_PORT}" 60 false; then
      NYM_STARTED=1
      log "Nym mixnet transport active via provider: ${provider:0:40}..."
      return 0
    else
      log "WARN: Nym init succeeded but SOCKS5 port did not open"
      # Kill the failed nym process
      local nym_pid="${PIDS[nym]:-}"
      if [[ -n "$nym_pid" ]] && kill -0 "$nym_pid" 2>/dev/null; then
        kill "$nym_pid" 2>/dev/null || true
        wait "$nym_pid" 2>/dev/null || true
      fi
      unset "PIDS[nym]"
      return 1
    fi
  else
    log "WARN: nym-socks5-client init failed for this provider"
    return 1
  fi
}

# Reuse existing identity if present and working
if [[ -d "$NYM_CONFIG_DIR" ]]; then
  log "Reusing existing Nym identity at ${NYM_CONFIG_DIR}"
  start_bg "nym" "${LOG_DIR}/nym.log" nym-socks5-client run --id "${NYM_ID}"
  if wait_for_port "Nym SOCKS5" "${NYM_PORT}" 60 false; then
    NYM_STARTED=1
    log "Nym mixnet transport active (existing identity)."
  else
    log "WARN: existing Nym identity failed to start; will re-init with discovery"
    _nym_pid="${PIDS[nym]:-}"
    if [[ -n "$_nym_pid" ]] && kill -0 "$_nym_pid" 2>/dev/null; then
      kill "$_nym_pid" 2>/dev/null || true
      wait "$_nym_pid" 2>/dev/null || true
    fi
    unset "PIDS[nym]"
    rm -rf "$NYM_CONFIG_DIR"
  fi
fi

# If not yet started, discover and try providers
if [[ "$NYM_STARTED" -eq 0 ]]; then
  # Temporarily clear proxy vars so discovery API calls can reach the internet
  _saved_http_proxy="${HTTP_PROXY:-}"
  _saved_http_proxy_lc="${http_proxy:-}"
  _saved_https_proxy="${HTTPS_PROXY:-}"
  _saved_https_proxy_lc="${https_proxy:-}"
  _saved_all_proxy="${ALL_PROXY:-}"
  _saved_all_proxy_lc="${all_proxy:-}"
  _saved_socks5_proxy="${SOCKS5_PROXY:-}"
  _saved_socks5_proxy_lc="${socks5_proxy:-}"
  _saved_no_proxy="${NO_PROXY:-}"
  _saved_no_proxy_lc="${no_proxy:-}"
  unset HTTP_PROXY HTTPS_PROXY ALL_PROXY SOCKS5_PROXY http_proxy https_proxy all_proxy socks5_proxy 2>/dev/null || true

  # Collect candidate providers: user-configured first, then auto-discovered
  PROVIDERS=()
  if [[ -n "${NYM_PROVIDER:-}" ]]; then
    PROVIDERS+=("${NYM_PROVIDER}")
  fi

  if [[ -x "${DISCOVER_SCRIPT}" ]]; then
    log "Discovering Nym providers from network..."
    while IFS= read -r addr; do
      [[ -n "${addr}" ]] && PROVIDERS+=("${addr}")
    done < <(NYM_PROVIDER="" "${DISCOVER_SCRIPT}" 2>>"${LOG_DIR}/nym_discovery.log" || true)
    log "Discovered ${#PROVIDERS[@]} candidate provider(s)"
  elif [[ ${#PROVIDERS[@]} -eq 0 ]]; then
    log "WARN: no NYM_PROVIDER set and discovery script not found"
  fi

  # scripts/nym-discover-provider.sh already deduplicates provider addresses.
  # Keep entrypoint ordering as collected (user-configured first, then discovery).

  # Try providers in order up to NYM_MAX_PROVIDER_ATTEMPTS
  # NOTE: proxy vars stay cleared during init — nym-socks5-client init needs
  # direct internet to reach validator.nymtech.net/api/
  attempt=0
  for provider in "${PROVIDERS[@]}"; do
    if [[ "$attempt" -ge "$NYM_MAX_PROVIDER_ATTEMPTS" ]]; then
      log "Reached max provider attempts (${NYM_MAX_PROVIDER_ATTEMPTS})"
      break
    fi
    attempt=$((attempt + 1))
    if try_nym_provider "$provider" "${attempt}/${NYM_MAX_PROVIDER_ATTEMPTS}"; then
      break
    fi
  done

  # Restore proxy vars now that Nym init/run is done
  # (if Nym started, traffic will route through it; if not, fail-open/closed handles it)
  [[ -n "$_saved_http_proxy" ]] && export HTTP_PROXY="$_saved_http_proxy"
  [[ -n "$_saved_http_proxy_lc" ]] && export http_proxy="$_saved_http_proxy_lc"
  [[ -n "$_saved_https_proxy" ]] && export HTTPS_PROXY="$_saved_https_proxy"
  [[ -n "$_saved_https_proxy_lc" ]] && export https_proxy="$_saved_https_proxy_lc"
  [[ -n "$_saved_all_proxy" ]] && export ALL_PROXY="$_saved_all_proxy"
  [[ -n "$_saved_all_proxy_lc" ]] && export all_proxy="$_saved_all_proxy_lc"
  [[ -n "$_saved_socks5_proxy" ]] && export SOCKS5_PROXY="$_saved_socks5_proxy"
  [[ -n "$_saved_socks5_proxy_lc" ]] && export socks5_proxy="$_saved_socks5_proxy_lc"
  [[ -n "$_saved_no_proxy" ]] && export NO_PROXY="$_saved_no_proxy"
  [[ -n "$_saved_no_proxy_lc" ]] && export no_proxy="$_saved_no_proxy_lc"
fi

# Handle failure
if [[ "$NYM_STARTED" -eq 0 ]]; then
  if [[ "${NYM_FAIL_OPEN:-false}" == "true" ]]; then
    log "WARNING: Nym transport failed to start. NYM_FAIL_OPEN=true — clearing proxy"
    log "env vars. Outbound connections will go DIRECT (no mixnet privacy)."
    unset SOCKS5_PROXY ALL_PROXY HTTP_PROXY HTTPS_PROXY socks5_proxy all_proxy http_proxy https_proxy
    export NO_PROXY="*"
    export no_proxy="*"
  else
    die "Nym transport failed to start and NYM_FAIL_OPEN=false. Cannot continue safely."
  fi
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

# -- Background CLI agent install (lazy, non-blocking) --
# Install to /data/npm-global (writable volume) so the read-only rootfs is fine.
# Dashboard is already serving; user can start setup while CLIs install.
# CLI downloads go DIRECT (no Nym proxy) — no privacy need for public npm registry.
CLI_LOG="${LOG_DIR}/cli_install.log"
(
  unset HTTP_PROXY HTTPS_PROXY ALL_PROXY SOCKS5_PROXY http_proxy https_proxy all_proxy socks5_proxy 2>/dev/null || true
  export NO_PROXY="*"
  export no_proxy="*"
  export NPM_CONFIG_PREFIX=/data/npm-global
  mkdir -p /data/npm-global
  for pkg in "@anthropic-ai/claude-code" "@openai/codex" "@google/gemini-cli" "openclaw@latest"; do
    if ! command -v "$(basename "${pkg%%@*}" | tr -d '@')" >/dev/null 2>&1; then
      log "Installing CLI: ${pkg} ..."
      if npm install -g "${pkg}" >>"${CLI_LOG}" 2>&1; then
        log "CLI installed: ${pkg}"
      else
        log "WARN: CLI install failed: ${pkg} (see ${CLI_LOG})"
      fi
    else
      log "CLI already installed: ${pkg}"
    fi
  done
  log "Background CLI install complete."
) &
CLI_INSTALL_PID=$!

while true; do
  EXITED_PID=""
  if wait -n -p EXITED_PID; then
    EXIT_CODE=0
  else
    EXIT_CODE=$?
  fi

  # Ignore the background CLI install subshell — it's not a critical service
  if [[ "$EXITED_PID" == "$CLI_INSTALL_PID" ]]; then
    continue
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
