#!/usr/bin/env bash
set -euo pipefail

# End-to-end multi-chain attestation verification on Base Sepolia.
#
# Required env:
#   RPC_URL_BASE_SEPOLIA
#   TRUST_VERIFIER_MULTI_CHAIN_ADDRESS
#   AGENT_ADDRESS
#
# Optional env:
#   USDC_BASE_SEPOLIA                 (default: read from contract)
#   TRUST_TREASURY                    (default: read from contract)
#   PROOF_HEX_SINGLE                  (default: 0x01)
#   PROOF_HEX_MULTI                   (default: 0x01)
#   PUBLIC_SIGNALS_SINGLE             (default: [1,2,3,4,2,1])
#   PUBLIC_SIGNALS_MULTI              (default: [1,2,3,4,2,2])
#   CHAINS_SINGLE                     (default: [8453])
#   CHAINS_MULTI                      (default: [8453,1])
#   REQUIRED_MULTI_PASS               (default: [8453,1])
#   REQUIRED_MULTI_FAIL               (default: [8453,42161])
#   DEPLOYMENT_OUT                    (optional; deployment evidence for hash chaining)
#   EVIDENCE_OUT                      (optional JSON evidence path)
#   ETH_KEYSTORE / ETH_PASSWORD_FILE  (preferred signer mode)
#   PRIVATE_KEY                       (fallback signer mode)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
EVIDENCE_OUT="${EVIDENCE_OUT:-}"
SCRIPT_SHA256="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 1
  fi
}

extract_tx_hash() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{64}' | grep -Evi '^0x0{64}$' | tail -n1
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

parse_uint_output() {
  local raw="$1"
  local token="${raw%% *}"
  if [[ "${token}" == 0x* ]]; then
    cast to-dec "${token}"
  else
    echo "${token}"
  fi
}

receipt_status() {
  local tx_hash="$1"
  local raw
  raw="$(cast rpc --rpc-url "${RPC_URL_BASE_SEPOLIA}" eth_getTransactionReceipt "${tx_hash}")"
  python3 - "$raw" <<'PY'
import json
import sys
obj = json.loads(sys.argv[1])
if not obj:
    print(-1)
    raise SystemExit(0)
status = obj.get("status")
if isinstance(status, str):
    print(int(status, 16) if status.startswith("0x") else int(status))
else:
    print(int(status))
PY
}

build_signer_args() {
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
}

run_send() {
  local selector="$1"
  shift
  cast send \
    --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${SIGNER_ARGS[@]}" \
    "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" \
    "${selector}" \
    "$@"
}

run_call() {
  local selector="$1"
  shift
  cast call \
    --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" \
    "${selector}" \
    "$@"
}

require_env "RPC_URL_BASE_SEPOLIA"
require_env "TRUST_VERIFIER_MULTI_CHAIN_ADDRESS"
require_env "AGENT_ADDRESS"

PROOF_HEX_SINGLE="${PROOF_HEX_SINGLE:-0x01}"
PROOF_HEX_MULTI="${PROOF_HEX_MULTI:-0x01}"
PUBLIC_SIGNALS_SINGLE="${PUBLIC_SIGNALS_SINGLE:-[1,2,3,4,2,1]}"
PUBLIC_SIGNALS_MULTI="${PUBLIC_SIGNALS_MULTI:-[1,2,3,4,2,2]}"
CHAINS_SINGLE="${CHAINS_SINGLE:-[8453]}"
CHAINS_MULTI="${CHAINS_MULTI:-[8453,1]}"
REQUIRED_MULTI_PASS="${REQUIRED_MULTI_PASS:-[8453,1]}"
REQUIRED_MULTI_FAIL="${REQUIRED_MULTI_FAIL:-[8453,42161]}"

build_signer_args

cd "${CONTRACTS_DIR}"

CHAIN_ID="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}")"
if [[ "${CHAIN_ID}" != "84532" ]]; then
  echo "unexpected chain id: ${CHAIN_ID} (expected 84532 Base Sepolia)" >&2
  exit 1
fi

if [[ -z "${USDC_BASE_SEPOLIA:-}" ]]; then
  USDC_BASE_SEPOLIA="$(run_call "usdc()(address)" | tr -d '[:space:]')"
fi
if [[ -z "${TRUST_TREASURY:-}" ]]; then
  TRUST_TREASURY="$(run_call "treasury()(address)" | tr -d '[:space:]')"
fi

BASE_CHAIN_INFO="$(run_call "chainInfo(uint256)(bool,address,bytes32,uint64,uint256)" "8453")"
ETH_CHAIN_INFO="$(run_call "chainInfo(uint256)(bool,address,bytes32,uint64,uint256)" "1")"
BASE_CHAIN_OK="$(normalize_bool_output "$(printf '%s\n' "${BASE_CHAIN_INFO}" | sed -n '1p')")"
ETH_CHAIN_OK="$(normalize_bool_output "$(printf '%s\n' "${ETH_CHAIN_INFO}" | sed -n '1p')")"
if [[ "${BASE_CHAIN_OK}" != "true" ]]; then
  echo "chain 8453 not registered in TrustVerifierMultiChain" >&2
  exit 1
fi
if [[ "${ETH_CHAIN_OK}" != "true" ]]; then
  echo "chain 1 not registered in TrustVerifierMultiChain" >&2
  exit 1
fi

PRE_HEX="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${USDC_BASE_SEPOLIA}" "balanceOf(address)(uint256)" "${TRUST_TREASURY}")"
PRE_BALANCE="$(parse_uint_output "${PRE_HEX}")"

DEFAULT_FEE_HEX="$(run_call "defaultFee()(uint256)")"
DEFAULT_FEE="$(parse_uint_output "${DEFAULT_FEE_HEX}")"
FEE_8453_HEX="$(run_call "chainFees(uint256)(uint256)" "8453")"
FEE_8453="$(parse_uint_output "${FEE_8453_HEX}")"
FEE_1_HEX="$(run_call "chainFees(uint256)(uint256)" "1")"
FEE_1="$(parse_uint_output "${FEE_1_HEX}")"
if [[ "${FEE_8453}" == "0" ]]; then FEE_8453="${DEFAULT_FEE}"; fi
if [[ "${FEE_1}" == "0" ]]; then FEE_1="${DEFAULT_FEE}"; fi
EXPECTED_DELTA=$((2 * FEE_8453 + FEE_1))

APPROVE_OUT="$(
  cast send \
    --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${SIGNER_ARGS[@]}" \
    "${USDC_BASE_SEPOLIA}" \
    "approve(address,uint256)" \
    "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" \
    "${EXPECTED_DELTA}"
)"
APPROVE_TX_HASH="$(extract_tx_hash "${APPROVE_OUT}")"
if [[ -z "${APPROVE_TX_HASH}" ]]; then
  echo "failed to parse approve tx hash" >&2
  exit 1
fi

SINGLE_OUT="$(run_send "submitCompositeAttestation(bytes,uint256[],uint256[])" "${PROOF_HEX_SINGLE}" "${PUBLIC_SIGNALS_SINGLE}" "${CHAINS_SINGLE}")"
SINGLE_TX_HASH="$(extract_tx_hash "${SINGLE_OUT}")"
if [[ -z "${SINGLE_TX_HASH}" ]]; then
  echo "failed to parse single-chain attestation tx hash" >&2
  exit 1
fi

MULTI_OUT="$(run_send "submitCompositeAttestation(bytes,uint256[],uint256[])" "${PROOF_HEX_MULTI}" "${PUBLIC_SIGNALS_MULTI}" "${CHAINS_MULTI}")"
MULTI_TX_HASH="$(extract_tx_hash "${MULTI_OUT}")"
if [[ -z "${MULTI_TX_HASH}" ]]; then
  echo "failed to parse multi-chain attestation tx hash" >&2
  exit 1
fi

SINGLE_RECEIPT_STATUS="$(receipt_status "${SINGLE_TX_HASH}")"
MULTI_RECEIPT_STATUS="$(receipt_status "${MULTI_TX_HASH}")"
if [[ "${SINGLE_RECEIPT_STATUS}" != "1" ]]; then
  echo "single-chain attestation receipt failed: ${SINGLE_RECEIPT_STATUS}" >&2
  exit 1
fi
if [[ "${MULTI_RECEIPT_STATUS}" != "1" ]]; then
  echo "multi-chain attestation receipt failed: ${MULTI_RECEIPT_STATUS}" >&2
  exit 1
fi

VERIFY_8453="$(normalize_bool_output "$(run_call "isVerifiedForChain(address,uint256)(bool)" "${AGENT_ADDRESS}" "8453")")"
VERIFY_1="$(normalize_bool_output "$(run_call "isVerifiedForChain(address,uint256)(bool)" "${AGENT_ADDRESS}" "1")")"
VERIFY_MULTI_PASS="$(normalize_bool_output "$(run_call "isVerifiedMultiChain(address,uint256[])(bool)" "${AGENT_ADDRESS}" "${REQUIRED_MULTI_PASS}")")"
VERIFY_MULTI_FAIL="$(normalize_bool_output "$(run_call "isVerifiedMultiChain(address,uint256[])(bool)" "${AGENT_ADDRESS}" "${REQUIRED_MULTI_FAIL}")")"

if [[ "${VERIFY_8453}" != "true" ]]; then
  echo "isVerifiedForChain(agent,8453) expected true" >&2
  exit 1
fi
if [[ "${VERIFY_1}" != "true" ]]; then
  echo "isVerifiedForChain(agent,1) expected true" >&2
  exit 1
fi
if [[ "${VERIFY_MULTI_PASS}" != "true" ]]; then
  echo "isVerifiedMultiChain(agent,[8453,1]) expected true" >&2
  exit 1
fi
if [[ "${VERIFY_MULTI_FAIL}" != "false" ]]; then
  echo "isVerifiedMultiChain(agent,[8453,42161]) expected false" >&2
  exit 1
fi

POST_HEX="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${USDC_BASE_SEPOLIA}" "balanceOf(address)(uint256)" "${TRUST_TREASURY}")"
POST_BALANCE="$(parse_uint_output "${POST_HEX}")"
DELTA=$((POST_BALANCE - PRE_BALANCE))
if [[ "${DELTA}" != "${EXPECTED_DELTA}" ]]; then
  echo "treasury delta mismatch: expected ${EXPECTED_DELTA}, got ${DELTA}" >&2
  exit 1
fi

REGISTERED_CHAIN_COUNT_RAW="$(run_call "registeredChainsLength()(uint256)")"
REGISTERED_CHAIN_COUNT="$(parse_uint_output "${REGISTERED_CHAIN_COUNT_RAW}")"
LIST_REGISTERED_RAW="$(run_call "getRegisteredChains()(uint256[])")"
DEPLOYMENT_EVIDENCE_SHA256=""
if [[ -n "${DEPLOYMENT_OUT:-}" && -f "${DEPLOYMENT_OUT}" ]]; then
  DEPLOYMENT_EVIDENCE_SHA256="$(sha256sum "${DEPLOYMENT_OUT}" | awk '{print $1}')"
fi

echo "PASS: multichain Base Sepolia e2e flow complete"
echo "  contract=${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}"
echo "  agent=${AGENT_ADDRESS}"
echo "  single_tx=${SINGLE_TX_HASH}"
echo "  multi_tx=${MULTI_TX_HASH}"
echo "  treasury_delta=${DELTA} (expected=${EXPECTED_DELTA})"
echo "  verify_8453=${VERIFY_8453}"
echo "  verify_1=${VERIFY_1}"
echo "  verify_multi_pass=${VERIFY_MULTI_PASS}"
echo "  verify_multi_fail=${VERIFY_MULTI_FAIL}"
echo "  registered_chain_count=${REGISTERED_CHAIN_COUNT}"

if [[ -n "${EVIDENCE_OUT}" ]]; then
  mkdir -p "$(dirname "${EVIDENCE_OUT}")"
  python3 - \
    "${EVIDENCE_OUT}" \
    "${CHAIN_ID}" \
    "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" \
    "${AGENT_ADDRESS}" \
    "${USDC_BASE_SEPOLIA}" \
    "${TRUST_TREASURY}" \
    "${APPROVE_TX_HASH}" \
    "${SINGLE_TX_HASH}" \
    "${MULTI_TX_HASH}" \
    "${SINGLE_RECEIPT_STATUS}" \
    "${MULTI_RECEIPT_STATUS}" \
    "${PRE_BALANCE}" \
    "${POST_BALANCE}" \
    "${DELTA}" \
    "${EXPECTED_DELTA}" \
    "${VERIFY_8453}" \
    "${VERIFY_1}" \
    "${VERIFY_MULTI_PASS}" \
    "${VERIFY_MULTI_FAIL}" \
    "${REGISTERED_CHAIN_COUNT}" \
    "${LIST_REGISTERED_RAW}" \
    "${SCRIPT_SHA256}" \
    "${DEPLOYMENT_EVIDENCE_SHA256}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    out_file,
    chain_id,
    contract_address,
    agent_address,
    usdc_address,
    treasury,
    approve_tx_hash,
    single_tx_hash,
    multi_tx_hash,
    single_receipt_status,
    multi_receipt_status,
    pre_balance,
    post_balance,
    delta,
    expected_delta,
    verify_8453,
    verify_1,
    verify_multi_pass,
    verify_multi_fail,
    registered_chain_count,
    list_registered_raw,
    script_sha256,
    deployment_evidence_sha256,
) = sys.argv[1:]

deployment_evidence_sha256 = deployment_evidence_sha256 or None

report = {
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "network": "base_sepolia",
    "script_sha256": script_sha256,
    "deployment_evidence_sha256": deployment_evidence_sha256,
    "chain_id": int(chain_id),
    "contract": contract_address,
    "agent": agent_address,
    "usdc": usdc_address,
    "treasury": treasury,
    "tx_hashes": {
        "approve": approve_tx_hash,
        "single_attestation": single_tx_hash,
        "multi_attestation": multi_tx_hash,
    },
    "receipts": {
        "single_status": int(single_receipt_status),
        "multi_status": int(multi_receipt_status),
    },
    "economics": {
        "pre_balance": int(pre_balance),
        "post_balance": int(post_balance),
        "delta": int(delta),
        "expected_delta": int(expected_delta),
    },
    "verification": {
        "chain_8453": verify_8453 == "true",
        "chain_1": verify_1 == "true",
        "multi_pass": verify_multi_pass == "true",
        "multi_fail_expected_false_observed": verify_multi_fail == "false",
    },
    "registered_chain_count": int(registered_chain_count),
    "registered_chains_raw": list_registered_raw.strip(),
}

with open(out_file, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2)
PY
  echo "  evidence_out=${EVIDENCE_OUT}"
fi
