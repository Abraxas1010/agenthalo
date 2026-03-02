#!/usr/bin/env bash
set -euo pipefail

NYM_ID="${NYM_ID:-agenthalo}"
NYM_PORT="${NYM_PORT:-1080}"
NYM_PROVIDER="${NYM_PROVIDER:?NYM_PROVIDER must be set to an active Nym network requester}"
NYM_DATA_DIR="${NYM_DATA_DIR:-/root/.nym}"
NYM_CONFIG_DIR="${NYM_DATA_DIR}/socks5-clients/${NYM_ID}"

mkdir -p "${NYM_DATA_DIR}"

if [ ! -d "${NYM_CONFIG_DIR}" ]; then
  echo "[AgentHalo/Nym] Initializing SOCKS5 client identity: ${NYM_ID}"
  nym-socks5-client init \
    --id "${NYM_ID}" \
    --provider "${NYM_PROVIDER}" \
    --port "${NYM_PORT}"
else
  echo "[AgentHalo/Nym] Reusing existing identity at ${NYM_CONFIG_DIR}"
fi

echo "[AgentHalo/Nym] Starting SOCKS5 client on port ${NYM_PORT}"
exec nym-socks5-client run --id "${NYM_ID}"
