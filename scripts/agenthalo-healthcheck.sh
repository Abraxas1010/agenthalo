#!/usr/bin/env bash
set -euo pipefail

DASHBOARD_PORT="${AGENTHALO_DASHBOARD_PORT:-3100}"
MCP_PORT="${AGENTHALO_MCP_PORT:-8390}"
NUCLEUSDB_PORT="${NUCLEUSDB_PORT:-8088}"
WDK_PORT="${WDK_PORT:-7321}"
NYM_PORT="${NYM_PORT:-1080}"

curl -sf "http://127.0.0.1:${DASHBOARD_PORT}/api/status" >/dev/null
curl -sf "http://127.0.0.1:${MCP_PORT}/health" >/dev/null
nc -z 127.0.0.1 "${NUCLEUSDB_PORT}" >/dev/null

if [[ -f /opt/wdk-sidecar/index.mjs ]]; then
  nc -z 127.0.0.1 "${WDK_PORT}" >/dev/null
fi

if [[ -n "${NYM_PROVIDER:-}" ]]; then
  nc -z 127.0.0.1 "${NYM_PORT}" >/dev/null
fi
