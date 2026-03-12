#!/usr/bin/env bash
set -euo pipefail

LOG_DIR="${NUCLEUSDB_HOME:-/data}/logs"
DASHBOARD_PORT="${NUCLEUSDB_DASHBOARD_PORT:-3100}"
MCP_PORT="${NUCLEUSDB_MCP_PORT:-3000}"
NUCLEUSDB_PORT="${NUCLEUSDB_API_PORT:-8088}"

export NUCLEUSDB_HOME="${NUCLEUSDB_HOME:-/data}"
export AGENTHALO_HOME="${AGENTHALO_HOME:-/data}"
export AGENTHALO_DASHBOARD_HOST="${AGENTHALO_DASHBOARD_HOST:-0.0.0.0}"
export NUCLEUSDB_MCP_HOST="${NUCLEUSDB_MCP_HOST:-0.0.0.0}"

log() { echo "[nucleusdb-container] $(date -u +%Y-%m-%dT%H:%M:%SZ) $*"; }

ensure_self_mesh_connect() {
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
  docker network inspect halo-mesh >/dev/null 2>&1 || docker network create --driver bridge --label nucleusdb.mesh=true halo-mesh >/dev/null 2>&1 || true
  if docker inspect --format '{{json .NetworkSettings.Networks}}' "${self_name}" 2>/dev/null | grep -q '"halo-mesh"'; then
    return 0
  fi
  docker network connect halo-mesh "${self_name}" >/dev/null 2>&1 || log "WARN: failed to attach ${self_name} to halo-mesh"
}

mkdir -p "${LOG_DIR}"

if [[ -S /var/run/docker.sock ]]; then
  log "WARN: host Docker socket is mounted; this container can manage host containers"
fi

ensure_self_mesh_connect

nucleusdb dashboard --port "${DASHBOARD_PORT}" --no-open >"${LOG_DIR}/dashboard.log" 2>&1 &
DASH_PID=$!

nucleusdb mcp --db /data/nucleusdb.ndb --transport http --host 0.0.0.0 --port "${MCP_PORT}" >"${LOG_DIR}/mcp.log" 2>&1 &
MCP_PID=$!

nucleusdb server --addr "0.0.0.0:${NUCLEUSDB_PORT}" --policy production >"${LOG_DIR}/server.log" 2>&1 &
API_PID=$!

term() {
  kill "${DASH_PID}" "${MCP_PID}" "${API_PID}" >/dev/null 2>&1 || true
  wait || true
}
trap term SIGTERM SIGINT EXIT

wait -n "${DASH_PID}" "${MCP_PID}" "${API_PID}"
