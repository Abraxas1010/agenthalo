#!/usr/bin/env bash
set -euo pipefail

# Deploy AgentHALO TrustVerifier on Base Sepolia with artifact emission.
#
# Required env:
#   RPC_URL_BASE_SEPOLIA
#   TRUST_VERIFIER_GROTH16_VERIFIER
#   TRUST_TREASURY
#
# Optional env:
#   USDC_BASE_SEPOLIA   (default Base Sepolia USDC)
#   TRUST_FEE_WEI       (default 0)
#   ETH_KEYSTORE + ETH_PASSWORD_FILE (preferred signer mode)
#   PRIVATE_KEY         (fallback signer mode)
#   BASESCAN_API_KEY    (if set, adds forge --verify)
#   DEPLOYMENT_OUT      (JSON artifact output path)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 1
  fi
}

extract_address() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{40}' | tail -n1 || true
}

extract_tx_hash() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{64}' | head -n1 || true
}

require_env "RPC_URL_BASE_SEPOLIA"
require_env "TRUST_VERIFIER_GROTH16_VERIFIER"
require_env "TRUST_TREASURY"

USDC_BASE_SEPOLIA="${USDC_BASE_SEPOLIA:-0x036CbD53842C5426634e7929541eC2318f3dCF7e}"
TRUST_FEE_WEI="${TRUST_FEE_WEI:-0}"
DEPLOYMENT_OUT="${DEPLOYMENT_OUT:-}"

CHAIN_ID="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}")"
if [[ "${CHAIN_ID}" != "84532" ]]; then
  echo "chain id mismatch: expected 84532, got ${CHAIN_ID}" >&2
  exit 1
fi

SIGNER_ARGS=()
SIGNER_MODE="unknown"
if [[ -n "${ETH_KEYSTORE:-}" ]]; then
  SIGNER_ARGS+=(--keystore "${ETH_KEYSTORE}")
  if [[ -n "${ETH_PASSWORD_FILE:-}" ]]; then
    SIGNER_ARGS+=(--password-file "${ETH_PASSWORD_FILE}")
  elif [[ -n "${ETH_PASSWORD:-}" ]]; then
    SIGNER_ARGS+=(--password-file "${ETH_PASSWORD}")
  else
    echo "keystore signer selected but ETH_PASSWORD_FILE/ETH_PASSWORD missing" >&2
    exit 1
  fi
  SIGNER_MODE="keystore"
elif [[ -n "${PRIVATE_KEY:-}" ]]; then
  SIGNER_ARGS+=(--private-key "${PRIVATE_KEY}")
  SIGNER_MODE="private_key"
else
  echo "missing signer credentials: set ETH_KEYSTORE (preferred) or PRIVATE_KEY" >&2
  exit 1
fi

VERIFY_ARGS=()
if [[ -n "${BASESCAN_API_KEY:-}" ]]; then
  VERIFY_ARGS+=(--verify --etherscan-api-key "${BASESCAN_API_KEY}")
fi

cd "${CONTRACTS_DIR}"
echo "Deploying TrustVerifier (chain_id=${CHAIN_ID}, signer=${SIGNER_MODE}) ..."

if ! CREATE_OUT="$(
  forge create \
    --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${SIGNER_ARGS[@]}" \
    "${VERIFY_ARGS[@]}" \
    TrustVerifier.sol:TrustVerifier \
    --constructor-args \
      "${TRUST_VERIFIER_GROTH16_VERIFIER}" \
      "${USDC_BASE_SEPOLIA}" \
      "${TRUST_TREASURY}" \
      "${TRUST_FEE_WEI}" \
    2>&1
)"; then
  echo "${CREATE_OUT}" >&2
  exit 1
fi

CONTRACT_ADDRESS="$(extract_address "${CREATE_OUT}")"
TX_HASH="$(extract_tx_hash "${CREATE_OUT}")"
if [[ -z "${CONTRACT_ADDRESS}" || -z "${TX_HASH}" ]]; then
  echo "failed to parse deployment output" >&2
  echo "${CREATE_OUT}" >&2
  exit 1
fi

SCRIPT_SHA256="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"
TIMESTAMP_UTC="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

echo "PASS: deployed TrustVerifier"
echo "  contract_address=${CONTRACT_ADDRESS}"
echo "  tx_hash=${TX_HASH}"
echo "  chain_id=${CHAIN_ID}"

if [[ -n "${DEPLOYMENT_OUT}" ]]; then
  mkdir -p "$(dirname "${DEPLOYMENT_OUT}")"
  python3 - "${DEPLOYMENT_OUT}" \
    "${TIMESTAMP_UTC}" \
    "${CHAIN_ID}" \
    "${CONTRACT_ADDRESS}" \
    "${TX_HASH}" \
    "${SCRIPT_SHA256}" \
    "${TRUST_VERIFIER_GROTH16_VERIFIER}" \
    "${USDC_BASE_SEPOLIA}" \
    "${TRUST_TREASURY}" \
    "${TRUST_FEE_WEI}" \
    "${SIGNER_MODE}" <<'PY'
import json
import sys

(
    out_file,
    timestamp_utc,
    chain_id,
    contract_address,
    tx_hash,
    script_sha256,
    verifier_address,
    usdc_address,
    treasury_address,
    fee_wei,
    signer_mode,
) = sys.argv[1:]

payload = {
    "schema": "agenthalo/phase5/deploy/v1",
    "timestamp_utc": timestamp_utc,
    "chain_id": int(chain_id),
    "contract_address": contract_address,
    "tx_hash": tx_hash,
    "script_sha256": script_sha256,
    "constructor": {
        "verifier": verifier_address,
        "usdc": usdc_address,
        "treasury": treasury_address,
        "fee_wei": int(fee_wei),
    },
    "signer_mode": signer_mode,
}
with open(out_file, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2)
PY
  echo "  deployment_out=${DEPLOYMENT_OUT}"
fi
