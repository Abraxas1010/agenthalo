#!/usr/bin/env bash
# nym-discover-provider.sh — discover a working Nym network requester provider.
#
# Queries the Nym mainnet node-status API for bonded gateways with high
# performance scores and active network requester addresses. Returns the
# best provider address on stdout, or exits non-zero if none found.
#
# Usage:
#   NYM_PROVIDER=$(./scripts/nym-discover-provider.sh)
#   # or with a preferred provider that gets tried first:
#   NYM_PROVIDER=$(NYM_PROVIDER="<addr>" ./scripts/nym-discover-provider.sh)
set -euo pipefail

NYM_STATUS_API="${NYM_STATUS_API:-https://mainnet-node-status-api.nymtech.cc/v2/gateways}"
NYM_DISCOVERY_PAGE_SIZE="${NYM_DISCOVERY_PAGE_SIZE:-100}"
NYM_DISCOVERY_MIN_PERF="${NYM_DISCOVERY_MIN_PERF:-90}"
NYM_DISCOVERY_TIMEOUT="${NYM_DISCOVERY_TIMEOUT:-15}"

log() { echo "[NymDiscovery] $*" >&2; }

# If NYM_PROVIDER is already set and non-empty, emit it as first candidate.
preferred="${NYM_PROVIDER:-}"

discover_providers() {
  local url="${NYM_STATUS_API}?size=${NYM_DISCOVERY_PAGE_SIZE}"
  local raw
  raw=$(curl -sf --max-time "${NYM_DISCOVERY_TIMEOUT}" "${url}" 2>/dev/null) || {
    log "WARN: could not reach Nym status API at ${NYM_STATUS_API}"
    return 1
  }

  # Extract bonded gateways with perf >= threshold, an advertised network requester
  # address, and active recent probe status for entry routing.
  # Container base is node:22, so Node.js is always available.
  if command -v node >/dev/null 2>&1; then
    echo "${raw}" | node -e "
const data = JSON.parse(require('fs').readFileSync('/dev/stdin','utf8'));
const minPerf = ${NYM_DISCOVERY_MIN_PERF};
const providers = (data.items || [])
  .filter(i => i.bonded && (i.performance || 0) >= minPerf)
  .filter(i => {
    const probe = i.last_probe_result || {};
    const asEntry = ((probe.outcome || {}).as_entry || {});
    return asEntry.can_connect === true && asEntry.can_route === true;
  })
  .map(i => ({ perf: i.performance || 0, addr: ((i.self_described || {}).network_requester || {}).address || '' }))
  .filter(p => p.addr)
  .sort((a, b) => b.perf - a.perf);
providers.forEach(p => console.log(p.addr));
"
  elif command -v python3 >/dev/null 2>&1; then
    echo "${raw}" | python3 -c "
import sys, json
data = json.load(sys.stdin)
min_perf = int(${NYM_DISCOVERY_MIN_PERF})
providers = []
for item in data.get('items', []):
    if not item.get('bonded'):
        continue
    perf = item.get('performance', 0)
    if perf < min_perf:
        continue
    probe = item.get('last_probe_result') or {}
    outcome = probe.get('outcome') if isinstance(probe, dict) else {}
    as_entry = (outcome or {}).get('as_entry') or {}
    if not (as_entry.get('can_connect') is True and as_entry.get('can_route') is True):
        continue
    sd = item.get('self_described') or {}
    nr = sd.get('network_requester') or {}
    addr = nr.get('address', '')
    if not addr:
        continue
    providers.append((perf, addr))
providers.sort(key=lambda x: -x[0])
for _, addr in providers:
    print(addr)
"
  elif command -v jq >/dev/null 2>&1; then
    echo "${raw}" | jq -r "
      .items[]
      | select(.bonded == true)
      | select(.performance >= ${NYM_DISCOVERY_MIN_PERF})
      | select((.last_probe_result.outcome.as_entry.can_connect // false) == true)
      | select((.last_probe_result.outcome.as_entry.can_route // false) == true)
      | .self_described.network_requester.address // empty
    " | head -20
  else
    log "ERROR: no JSON parser available (need node, python3, or jq)"
    return 1
  fi
}

# Collect candidates: preferred first, then discovered.
candidates=()
if [[ -n "${preferred}" ]]; then
  candidates+=("${preferred}")
fi

log "Querying Nym network for available providers..."
while IFS= read -r addr; do
  [[ -n "${addr}" ]] && candidates+=("${addr}")
done < <(discover_providers 2>/dev/null || true)

if [[ ${#candidates[@]} -eq 0 ]]; then
  log "ERROR: no Nym providers discovered and no NYM_PROVIDER configured"
  exit 1
fi

log "Found ${#candidates[@]} candidate provider(s)"

# Emit all candidates (one per line). The entrypoint iterates and tries
# each in order, up to NYM_MAX_PROVIDER_ATTEMPTS.
declare -A seen=()
for addr in "${candidates[@]}"; do
  [[ -n "${addr}" ]] || continue
  if [[ -z "${seen[$addr]+x}" ]]; then
    seen["$addr"]=1
    echo "${addr}"
  fi
done
