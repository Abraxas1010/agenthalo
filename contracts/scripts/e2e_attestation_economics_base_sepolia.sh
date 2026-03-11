#!/usr/bin/env bash
set -euo pipefail

# Base Sepolia economics check for deployed TrustVerifier.
# Performs approve + attestAndPay + pre/post treasury balance verification.
#
# Optional env:
#   EVIDENCE_OUT  (JSON evidence output path)

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 1
  fi
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
EVIDENCE_OUT="${EVIDENCE_OUT:-}"

parse_uint_output() {
  local raw="$1"
  local token="${raw%% *}"
  if [[ "${token}" == 0x* ]]; then
    cast to-dec "${token}"
  else
    echo "${token}"
  fi
}

extract_tx_hash() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{64}' | head -n1
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

require_env "RPC_URL_BASE_SEPOLIA"
require_env "TRUST_VERIFIER_ADDRESS"
require_env "AGENT_ADDRESS"

TRUST_FEE_WEI="${TRUST_FEE_WEI:-0}"
PROOF_HEX="${PROOF_HEX:-0x01}"
PUBLIC_SIGNALS="${PUBLIC_SIGNALS:-[1,2,3,4,2,1]}"

SIGNER_ARGS=()
if [[ -n "${ETH_KEYSTORE:-}" ]]; then
  SIGNER_ARGS+=(--keystore "${ETH_KEYSTORE}")
  if [[ -n "${ETH_PASSWORD_FILE:-}" ]]; then
    SIGNER_ARGS+=(--password-file "${ETH_PASSWORD_FILE}")
  elif [[ -n "${ETH_PASSWORD:-}" ]]; then
    SIGNER_ARGS+=(--password-file "${ETH_PASSWORD}")
  fi
elif [[ -n "${PRIVATE_KEY:-}" ]]; then
  SIGNER_ARGS+=(--private-key "${PRIVATE_KEY}")
else
  echo "missing signer credentials: set ETH_KEYSTORE (preferred) or PRIVATE_KEY" >&2
  exit 1
fi

cd "${CONTRACTS_DIR}"

if [[ "${TRUST_FEE_WEI}" == "0" ]]; then
  TRUST_FEE_WEI_RAW=$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${TRUST_VERIFIER_ADDRESS}" "fee()(uint256)")
  TRUST_FEE_WEI=$(parse_uint_output "${TRUST_FEE_WEI_RAW}")
fi

if [[ -z "${USDC_BASE_SEPOLIA:-}" ]]; then
  USDC_BASE_SEPOLIA="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "usdc()(address)" | tr -d '[:space:]')"
fi
if [[ -z "${TRUST_TREASURY:-}" ]]; then
  TRUST_TREASURY="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "treasury()(address)" | tr -d '[:space:]')"
fi

echo "Checking pre-balance ..."
PRE_HEX=$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
  "${USDC_BASE_SEPOLIA}" "balanceOf(address)(uint256)" "${TRUST_TREASURY}")
PRE=$(parse_uint_output "${PRE_HEX}")

if [[ "${TRUST_FEE_WEI}" != "0" ]]; then
  echo "Approving USDC spend ..."
  APPROVE_SEND_OUT=$(cast send --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${SIGNER_ARGS[@]}" \
    "${USDC_BASE_SEPOLIA}" "approve(address,uint256)" "${TRUST_VERIFIER_ADDRESS}" "${TRUST_FEE_WEI}")
  APPROVE_TX_HASH="$(extract_tx_hash "${APPROVE_SEND_OUT}")"
  if [[ -z "${APPROVE_TX_HASH:-}" ]]; then
    echo "failed to parse approve tx hash" >&2
    exit 1
  fi
else
  APPROVE_TX_HASH=""
fi

echo "Submitting attestAndPay ..."
ATTEST_SEND_OUT=$(cast send --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${SIGNER_ARGS[@]}" \
  "${TRUST_VERIFIER_ADDRESS}" "attestAndPay(bytes,uint256[])" "${PROOF_HEX}" "${PUBLIC_SIGNALS}")
ATTEST_TX_HASH="$(extract_tx_hash "${ATTEST_SEND_OUT}")"
if [[ -z "${ATTEST_TX_HASH:-}" ]]; then
  echo "failed to parse attestation tx hash" >&2
  exit 1
fi

RECEIPT_JSON=$(cast rpc --rpc-url "${RPC_URL_BASE_SEPOLIA}" eth_getTransactionReceipt "${ATTEST_TX_HASH}")
read -r RECEIPT_STATUS RECEIPT_BLOCK <<<"$(python3 - "${RECEIPT_JSON}" <<'PY'
import json
import sys
raw = sys.argv[1]
obj = json.loads(raw)
if not obj:
    print("-1 -1")
    raise SystemExit(0)
status = obj.get("status")
block = obj.get("blockNumber")
def to_int(v):
    if v is None:
        return -1
    if isinstance(v, str):
        return int(v, 16) if v.startswith("0x") else int(v)
    return int(v)
print(f"{to_int(status)} {to_int(block)}")
PY
)"
if [[ "${RECEIPT_STATUS}" != "1" ]]; then
  echo "attestation receipt status not successful: ${RECEIPT_STATUS}" >&2
  exit 1
fi

echo "Checking post-balance and status ..."
POST_HEX=$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
  "${USDC_BASE_SEPOLIA}" "balanceOf(address)(uint256)" "${TRUST_TREASURY}")
POST=$(parse_uint_output "${POST_HEX}")
DELTA=$((POST - PRE))

VERIFY_AGENT_RAW=$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
  "${TRUST_VERIFIER_ADDRESS}" "verifyAgent(address)(bool)" "${AGENT_ADDRESS}")
VERIFY_AGENT=$(normalize_bool_output "${VERIFY_AGENT_RAW}")

if [[ "${TRUST_FEE_WEI}" != "0" && "${DELTA}" != "${TRUST_FEE_WEI}" ]]; then
  echo "fee delta mismatch: expected ${TRUST_FEE_WEI}, got ${DELTA}" >&2
  exit 1
fi

if [[ "${VERIFY_AGENT}" != "true" ]]; then
  echo "verifyAgent returned ${VERIFY_AGENT}, expected true" >&2
  exit 1
fi

echo "PASS: Base Sepolia economics flow complete"
echo "  pre_balance=${PRE}"
echo "  post_balance=${POST}"
echo "  delta=${DELTA}"
echo "  verify_agent=${VERIFY_AGENT}"
echo "  approve_tx_hash=${APPROVE_TX_HASH:-none}"
echo "  attest_tx_hash=${ATTEST_TX_HASH}"
echo "  attest_receipt_status=${RECEIPT_STATUS}"
echo "  attest_block_number=${RECEIPT_BLOCK}"

if [[ -n "${EVIDENCE_OUT}" ]]; then
  mkdir -p "$(dirname "${EVIDENCE_OUT}")"
  CHAIN_ID=$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}")
  python3 - \
    "${EVIDENCE_OUT}" \
    "${CHAIN_ID}" \
    "${TRUST_VERIFIER_ADDRESS}" \
    "${USDC_BASE_SEPOLIA}" \
    "${TRUST_TREASURY}" \
    "${AGENT_ADDRESS}" \
    "${TRUST_FEE_WEI}" \
    "${PRE}" \
    "${POST}" \
    "${DELTA}" \
    "${VERIFY_AGENT}" \
    "${APPROVE_TX_HASH}" \
    "${ATTEST_TX_HASH}" \
    "${RECEIPT_STATUS}" \
    "${RECEIPT_BLOCK}" <<'PY'
import json
import sys
from datetime import datetime, timezone
(
    out_file,
    chain_id,
    trust_verifier_address,
    usdc_address,
    treasury,
    agent,
    fee_wei,
    pre_balance,
    post_balance,
    delta,
    verify_agent,
    approve_tx_hash,
    attest_tx_hash,
    receipt_status,
    receipt_block,
) = sys.argv[1:]
report = {
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "network": "base_sepolia",
    "chain_id": int(chain_id),
    "contracts": {
        "trust_verifier": trust_verifier_address,
        "usdc": usdc_address,
    },
    "participants": {
        "treasury": treasury,
        "agent": agent,
    },
    "economics": {
        "fee_wei": int(fee_wei),
        "pre_balance": int(pre_balance),
        "post_balance": int(post_balance),
        "delta": int(delta),
    },
    "verification": {
        "verify_agent": verify_agent == "true",
        "receipt_status": int(receipt_status),
        "receipt_block": int(receipt_block),
    },
    "tx_hashes": {
        "approve": approve_tx_hash if approve_tx_hash else None,
        "attest_and_pay": attest_tx_hash,
    },
}
with open(out_file, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2)
PY
  echo "  evidence_out=${EVIDENCE_OUT}"
fi
