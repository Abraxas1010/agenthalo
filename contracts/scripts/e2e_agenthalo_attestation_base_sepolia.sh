#!/usr/bin/env bash
set -euo pipefail

# End-to-end AgentHALO on-chain attestation flow:
#  - configure on-chain settings
#  - run non-anonymous + anonymous attestations via CLI
#  - verify digest status and fetch attestation records
#  - emit JSON evidence linked to deployment artifact hash
#
# Required env:
#   RPC_URL_BASE_SEPOLIA
#   TRUST_VERIFIER_ADDRESS
#
# Optional env:
#   AGENTHALO_BIN                    (default: target/debug/agenthalo, then target/release/agenthalo)
#   AGENTHALO_HOME                   (default: .tmp/agenthalo_phase5_e2e)
#   AGENTHALO_ONCHAIN_SIMULATION     (default: 1)
#   AGENTHALO_ONCHAIN_PRIVATE_KEY    (used when signer-mode private_key_env and non-simulation mode)
#   ETH_KEYSTORE + ETH_PASSWORD_FILE (used when signer-mode keystore)
#   AGENT_ADDRESS                    (optional; auto-derived from signer if unset)
#   DEPLOYMENT_OUT                   (optional deployment artifact path for hash chaining)
#   EVIDENCE_OUT                     (JSON output path)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 1
  fi
}

extract_json_field() {
  local file="$1"
  local field="$2"
  python3 - "${file}" "${field}" <<'PY'
import json
import sys

path = sys.argv[1]
field = sys.argv[2].split(".")
text = open(path, "r", encoding="utf-8").read()
decoder = json.JSONDecoder()
obj = None
for i, ch in enumerate(text):
    if ch != "{":
        continue
    try:
        candidate, _ = decoder.raw_decode(text[i:])
    except json.JSONDecodeError:
        continue
    obj = candidate
    break
if obj is None:
    raise SystemExit(1)
cur = obj
for part in field:
    if part not in cur:
        raise SystemExit(2)
    cur = cur[part]
if cur is None:
    print("")
elif isinstance(cur, bool):
    print("true" if cur else "false")
else:
    print(cur)
PY
}

extract_tx_hash() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{64}' | head -n1 || true
}

normalize_bool_output() {
  local raw="$1"
  local token
  token="$(printf '%s' "${raw}" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')"
  case "${token}" in
    true|1|0x1) echo "true" ;;
    false|0|0x0) echo "false" ;;
    *) echo "${token}" ;;
  esac
}

normalize_hex32() {
  local raw="$1"
  local t="${raw//[[:space:]]/}"
  if [[ "${t}" == 0x* ]]; then
    echo "${t}"
  else
    echo "0x${t}"
  fi
}

receipt_status_and_gas() {
  local tx_hash="$1"
  local rpc_url="$2"
  python3 - "${rpc_url}" "${tx_hash}" <<'PY'
import json
import subprocess
import sys

rpc_url, tx_hash = sys.argv[1:]
raw = subprocess.check_output(
    ["cast", "rpc", "--rpc-url", rpc_url, "eth_getTransactionReceipt", tx_hash],
    text=True,
)
obj = json.loads(raw)
def to_int(v):
    if v is None:
        return -1
    if isinstance(v, str):
        return int(v, 16) if v.startswith("0x") else int(v)
    return int(v)
if not obj:
    print("-1 -1")
    raise SystemExit(0)
print(f"{to_int(obj.get('status'))} {to_int(obj.get('gasUsed'))}")
PY
}

require_env "RPC_URL_BASE_SEPOLIA"
require_env "TRUST_VERIFIER_ADDRESS"

CHAIN_ID="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}")"
if [[ "${CHAIN_ID}" != "84532" ]]; then
  echo "chain id mismatch: expected 84532, got ${CHAIN_ID}" >&2
  exit 1
fi

if [[ -n "${AGENTHALO_BIN:-}" ]]; then
  BIN="${AGENTHALO_BIN}"
elif [[ -x "${REPO_DIR}/target/debug/agenthalo" ]]; then
  BIN="${REPO_DIR}/target/debug/agenthalo"
elif [[ -x "${REPO_DIR}/target/release/agenthalo" ]]; then
  BIN="${REPO_DIR}/target/release/agenthalo"
else
  echo "agenthalo binary not found; set AGENTHALO_BIN or build target/debug/agenthalo" >&2
  exit 1
fi

export AGENTHALO_HOME="${AGENTHALO_HOME:-${REPO_DIR}/.tmp/agenthalo_phase5_e2e}"
mkdir -p "${AGENTHALO_HOME}"
export AGENTHALO_API_KEY="${AGENTHALO_API_KEY:-phase5-dev-api-key}"
export AGENTHALO_ALLOW_GENERIC="${AGENTHALO_ALLOW_GENERIC:-1}"
export AGENTHALO_AGENTPMT_SIMULATION="${AGENTHALO_AGENTPMT_SIMULATION:-1}"
export AGENTHALO_ONCHAIN_SIMULATION="${AGENTHALO_ONCHAIN_SIMULATION:-1}"
export AGENTHALO_ONCHAIN_PRIVATE_KEY="${AGENTHALO_ONCHAIN_PRIVATE_KEY:-${PRIVATE_KEY:-}}"

mkdir -p "${CONTRACTS_DIR}/artifacts/ops/agenthalo_phase5"
RUN_TS="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${CONTRACTS_DIR}/artifacts/ops/agenthalo_phase5/run_${RUN_TS}"
mkdir -p "${RUN_DIR}"

"${BIN}" config set-agentpmt-key phase5-test-key >/dev/null

ONCHAIN_CONFIG_ARGS=(
  onchain config
  --rpc-url "${RPC_URL_BASE_SEPOLIA}"
  --chain-id "${CHAIN_ID}"
  --contract "${TRUST_VERIFIER_ADDRESS}"
  --circuit-policy "${CIRCUIT_POLICY:-dev}"
)

if [[ -n "${ETH_KEYSTORE:-}" && -n "${ETH_PASSWORD_FILE:-}" ]]; then
  ONCHAIN_CONFIG_ARGS+=(
    --signer-mode keystore
    --keystore-path "${ETH_KEYSTORE}"
    --keystore-password-file "${ETH_PASSWORD_FILE}"
  )
else
  ONCHAIN_CONFIG_ARGS+=(--signer-mode private_key_env --private-key-env AGENTHALO_ONCHAIN_PRIVATE_KEY)
fi

"${BIN}" "${ONCHAIN_CONFIG_ARGS[@]}" >/dev/null

"${BIN}" run /bin/echo "phase5-e2e" >/dev/null
SESSION_ID="$("${BIN}" traces | awk -F'|' 'NR>2{gsub(/ /,"",$1); if(length($1)>0){print $1; exit}}')"
if [[ -z "${SESSION_ID}" ]]; then
  echo "failed to resolve session id from agenthalo traces" >&2
  exit 1
fi

NON_ANON_OUT="${RUN_DIR}/attest_nonanon.out"
ANON_OUT="${RUN_DIR}/attest_anon.out"
"${BIN}" attest --session "${SESSION_ID}" --onchain >"${NON_ANON_OUT}"
"${BIN}" attest --session "${SESSION_ID}" --onchain --anonymous >"${ANON_OUT}"

DIGEST_NON_ANON="$(normalize_hex32 "$(extract_json_field "${NON_ANON_OUT}" "attestation_digest")")"
DIGEST_ANON="$(normalize_hex32 "$(extract_json_field "${ANON_OUT}" "attestation_digest")")"
TX_NON_ANON="$(extract_json_field "${NON_ANON_OUT}" "tx_hash")"
TX_ANON="$(extract_json_field "${ANON_OUT}" "tx_hash")"

VERIFY_NON_ANON_RAW="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "isVerified(bytes32)(bool)" "${DIGEST_NON_ANON}" 2>/dev/null || true)"
VERIFY_ANON_RAW="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "isVerified(bytes32)(bool)" "${DIGEST_ANON}" 2>/dev/null || true)"
VERIFY_NON_ANON="$(normalize_bool_output "${VERIFY_NON_ANON_RAW}")"
VERIFY_ANON="$(normalize_bool_output "${VERIFY_ANON_RAW}")"

RECORD_NON_ANON="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "getAttestation(bytes32)((bytes32,bytes32,uint64,address,uint64,bool))" "${DIGEST_NON_ANON}" 2>/dev/null || true)"
RECORD_ANON="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "getAttestation(bytes32)((bytes32,bytes32,uint64,address,uint64,bool))" "${DIGEST_ANON}" 2>/dev/null || true)"

if [[ "${AGENTHALO_ONCHAIN_SIMULATION}" == "1" || "${AGENTHALO_ONCHAIN_SIMULATION}" == "true" ]]; then
  RECEIPT_NON_ANON_STATUS=-1
  RECEIPT_NON_ANON_GAS=-1
  RECEIPT_ANON_STATUS=-1
  RECEIPT_ANON_GAS=-1
else
  read -r RECEIPT_NON_ANON_STATUS RECEIPT_NON_ANON_GAS <<<"$(receipt_status_and_gas "${TX_NON_ANON}" "${RPC_URL_BASE_SEPOLIA}")"
  read -r RECEIPT_ANON_STATUS RECEIPT_ANON_GAS <<<"$(receipt_status_and_gas "${TX_ANON}" "${RPC_URL_BASE_SEPOLIA}")"
fi

DEPLOYMENT_EVIDENCE_SHA256=""
if [[ -n "${DEPLOYMENT_OUT:-}" && -f "${DEPLOYMENT_OUT}" ]]; then
  DEPLOYMENT_EVIDENCE_SHA256="$(sha256sum "${DEPLOYMENT_OUT}" | awk '{print $1}')"
fi
SCRIPT_SHA256="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"
EVIDENCE_OUT="${EVIDENCE_OUT:-${RUN_DIR}/e2e_evidence.json}"

python3 - "${EVIDENCE_OUT}" \
  "${RUN_TS}" \
  "${CHAIN_ID}" \
  "${TRUST_VERIFIER_ADDRESS}" \
  "${SESSION_ID}" \
  "${DIGEST_NON_ANON}" \
  "${DIGEST_ANON}" \
  "${TX_NON_ANON}" \
  "${TX_ANON}" \
  "${VERIFY_NON_ANON}" \
  "${VERIFY_ANON}" \
  "${RECEIPT_NON_ANON_STATUS}" \
  "${RECEIPT_NON_ANON_GAS}" \
  "${RECEIPT_ANON_STATUS}" \
  "${RECEIPT_ANON_GAS}" \
  "${RECORD_NON_ANON}" \
  "${RECORD_ANON}" \
  "${SCRIPT_SHA256}" \
  "${DEPLOYMENT_EVIDENCE_SHA256}" \
  "${AGENTHALO_ONCHAIN_SIMULATION}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    out_file,
    run_ts,
    chain_id,
    contract_address,
    session_id,
    digest_non_anon,
    digest_anon,
    tx_non_anon,
    tx_anon,
    verify_non_anon,
    verify_anon,
    receipt_non_anon_status,
    receipt_non_anon_gas,
    receipt_anon_status,
    receipt_anon_gas,
    record_non_anon,
    record_anon,
    script_sha256,
    deployment_evidence_sha256,
    simulation_mode,
) = sys.argv[1:]

payload = {
    "schema": "agenthalo/phase5/e2e/v1",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "run_id": run_ts,
    "chain_id": int(chain_id),
    "contract_address": contract_address,
    "session_id": session_id,
    "simulation_mode": simulation_mode.lower() in {"1", "true", "yes"},
    "non_anonymous": {
        "attestation_digest": digest_non_anon,
        "tx_hash": tx_non_anon,
        "is_verified": verify_non_anon == "true",
        "receipt_status": int(receipt_non_anon_status),
        "gas_used": int(receipt_non_anon_gas),
        "attestation_record_raw": record_non_anon,
    },
    "anonymous": {
        "attestation_digest": digest_anon,
        "tx_hash": tx_anon,
        "is_verified": verify_anon == "true",
        "receipt_status": int(receipt_anon_status),
        "gas_used": int(receipt_anon_gas),
        "attestation_record_raw": record_anon,
    },
    "script_sha256": script_sha256,
    "deployment_evidence_sha256": deployment_evidence_sha256 or None,
}

with open(out_file, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2)
PY

echo "PASS: AgentHALO Phase 5 E2E complete"
echo "  session_id=${SESSION_ID}"
echo "  digest_non_anon=${DIGEST_NON_ANON}"
echo "  digest_anon=${DIGEST_ANON}"
echo "  verify_non_anon=${VERIFY_NON_ANON}"
echo "  verify_anon=${VERIFY_ANON}"
echo "  evidence_out=${EVIDENCE_OUT}"
