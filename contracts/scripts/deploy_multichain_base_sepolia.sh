#!/usr/bin/env bash
set -euo pipefail

# Deploy TrustVerifierMultiChain on Base Sepolia and optionally register baseline chains.
#
# Required env:
#   RPC_URL_BASE_SEPOLIA
#   TRUST_VERIFIER_GROTH16_VERIFIER
#   TRUST_TREASURY
#
# Optional env:
#   USDC_BASE_SEPOLIA                  (default: Base Sepolia USDC)
#   TRUST_LEGACY_FEE_WEI               (default: 0)
#   TRUST_DEFAULT_FEE_WEI              (default: 1000000)
#   TRUST_BASE_CHAIN_FEE_WEI           (default: 1000000)
#   TRUST_ETH_CHAIN_FEE_WEI            (default: 5000000)
#   TRUST_ARB_CHAIN_FEE_WEI            (default: 2000000)
#   REGISTER_BASE_CHAIN                (default: true)
#   REGISTER_ETH_CHAIN                 (default: true)
#   REGISTER_ARB_CHAIN                 (default: false)
#   CHAIN_8453_VERIFIER                (default: TRUST_VERIFIER_GROTH16_VERIFIER)
#   CHAIN_1_VERIFIER                   (default: TRUST_VERIFIER_GROTH16_VERIFIER)
#   CHAIN_42161_VERIFIER               (default: TRUST_VERIFIER_GROTH16_VERIFIER)
#   BASESCAN_API_KEY                   (optional; enables forge --verify)
#   DEPLOYMENT_OUT                     (optional JSON report path)
#   ETH_KEYSTORE / ETH_PASSWORD_FILE   (preferred signer mode)
#   PRIVATE_KEY                        (fallback signer mode)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEPLOYMENT_OUT="${DEPLOYMENT_OUT:-}"
SCRIPT_SHA256="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 1
  fi
}

extract_address() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{40}' | head -n1
}

extract_tx_hash() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{64}' | head -n1
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
  local contract="$1"
  local selector="$2"
  shift 2
  cast send \
    --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${SIGNER_ARGS[@]}" \
    "${contract}" \
    "${selector}" \
    "$@"
}

register_chain() {
  local contract="$1"
  local chain_id="$2"
  local verifier="$3"
  local fee="$4"
  local metadata_hash
  metadata_hash=$(cast keccak "nucleusdb.multichain.registry.v1|${chain_id}|${verifier}")

  local reg_out
  reg_out="$(run_send "${contract}" "registerChain(uint256,address,bytes32)" "${chain_id}" "${verifier}" "${metadata_hash}")"
  local reg_tx
  reg_tx="$(extract_tx_hash "${reg_out}")"
  if [[ -z "${reg_tx}" ]]; then
    echo "failed to parse registerChain tx hash for chain ${chain_id}" >&2
    exit 1
  fi

  local fee_out
  fee_out="$(run_send "${contract}" "setChainFee(uint256,uint256)" "${chain_id}" "${fee}")"
  local fee_tx
  fee_tx="$(extract_tx_hash "${fee_out}")"
  if [[ -z "${fee_tx}" ]]; then
    echo "failed to parse setChainFee tx hash for chain ${chain_id}" >&2
    exit 1
  fi

  echo "${chain_id}:${reg_tx}:${fee_tx}"
}

require_env "RPC_URL_BASE_SEPOLIA"
require_env "TRUST_VERIFIER_GROTH16_VERIFIER"
require_env "TRUST_TREASURY"

USDC_BASE_SEPOLIA="${USDC_BASE_SEPOLIA:-0x036CbD53842C5426634e7929541eC2318f3dCF7e}"
TRUST_LEGACY_FEE_WEI="${TRUST_LEGACY_FEE_WEI:-0}"
TRUST_DEFAULT_FEE_WEI="${TRUST_DEFAULT_FEE_WEI:-1000000}"
TRUST_BASE_CHAIN_FEE_WEI="${TRUST_BASE_CHAIN_FEE_WEI:-1000000}"
TRUST_ETH_CHAIN_FEE_WEI="${TRUST_ETH_CHAIN_FEE_WEI:-5000000}"
TRUST_ARB_CHAIN_FEE_WEI="${TRUST_ARB_CHAIN_FEE_WEI:-2000000}"

REGISTER_BASE_CHAIN="${REGISTER_BASE_CHAIN:-true}"
REGISTER_ETH_CHAIN="${REGISTER_ETH_CHAIN:-true}"
REGISTER_ARB_CHAIN="${REGISTER_ARB_CHAIN:-false}"

CHAIN_8453_VERIFIER="${CHAIN_8453_VERIFIER:-${TRUST_VERIFIER_GROTH16_VERIFIER}}"
CHAIN_1_VERIFIER="${CHAIN_1_VERIFIER:-${TRUST_VERIFIER_GROTH16_VERIFIER}}"
CHAIN_42161_VERIFIER="${CHAIN_42161_VERIFIER:-${TRUST_VERIFIER_GROTH16_VERIFIER}}"

build_signer_args

cd "${CONTRACTS_DIR}"

CHAIN_ID="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}")"
if [[ "${CHAIN_ID}" != "84532" ]]; then
  echo "unexpected chain id: ${CHAIN_ID} (expected 84532 Base Sepolia)" >&2
  exit 1
fi

echo "Deploying TrustVerifierMultiChain to Base Sepolia..."
VERIFY_ARGS=()
if [[ -n "${BASESCAN_API_KEY:-}" ]]; then
  VERIFY_ARGS+=(--verify --etherscan-api-key "${BASESCAN_API_KEY}")
fi
DEPLOY_OUT="$(
  forge create \
    --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
    "${SIGNER_ARGS[@]}" \
    "${VERIFY_ARGS[@]}" \
    TrustVerifierMultiChain.sol:TrustVerifierMultiChain \
    --constructor-args \
      "${TRUST_VERIFIER_GROTH16_VERIFIER}" \
      "${USDC_BASE_SEPOLIA}" \
      "${TRUST_TREASURY}" \
      "${TRUST_LEGACY_FEE_WEI}" \
      "${TRUST_DEFAULT_FEE_WEI}"
)"
TRUST_VERIFIER_MULTI_CHAIN_ADDRESS="$(extract_address "${DEPLOY_OUT}")"
if [[ -z "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" ]]; then
  echo "failed to parse deployed contract address" >&2
  exit 1
fi

DEFAULT_FEE_TX_OUT="$(run_send "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "setDefaultFee(uint256)" "${TRUST_DEFAULT_FEE_WEI}")"
DEFAULT_FEE_TX_HASH="$(extract_tx_hash "${DEFAULT_FEE_TX_OUT}")"
if [[ -z "${DEFAULT_FEE_TX_HASH}" ]]; then
  echo "failed to parse setDefaultFee tx hash" >&2
  exit 1
fi

REGISTERED=()
if [[ "${REGISTER_BASE_CHAIN}" == "true" ]]; then
  REGISTERED+=("$(register_chain "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "8453" "${CHAIN_8453_VERIFIER}" "${TRUST_BASE_CHAIN_FEE_WEI}")")
fi
if [[ "${REGISTER_ETH_CHAIN}" == "true" ]]; then
  REGISTERED+=("$(register_chain "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "1" "${CHAIN_1_VERIFIER}" "${TRUST_ETH_CHAIN_FEE_WEI}")")
fi
if [[ "${REGISTER_ARB_CHAIN}" == "true" ]]; then
  REGISTERED+=("$(register_chain "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "42161" "${CHAIN_42161_VERIFIER}" "${TRUST_ARB_CHAIN_FEE_WEI}")")
fi

echo "PASS: TrustVerifierMultiChain deployed"
echo "  chain_id=${CHAIN_ID}"
echo "  contract=${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}"
echo "  default_fee_tx=${DEFAULT_FEE_TX_HASH}"
if [[ ${#REGISTERED[@]} -gt 0 ]]; then
  printf '  registered=%s\n' "${REGISTERED[@]}"
fi

if [[ -n "${DEPLOYMENT_OUT}" ]]; then
  mkdir -p "$(dirname "${DEPLOYMENT_OUT}")"
  python3 - \
    "${DEPLOYMENT_OUT}" \
    "${CHAIN_ID}" \
    "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" \
    "${TRUST_VERIFIER_GROTH16_VERIFIER}" \
    "${USDC_BASE_SEPOLIA}" \
    "${TRUST_TREASURY}" \
    "${TRUST_LEGACY_FEE_WEI}" \
    "${TRUST_DEFAULT_FEE_WEI}" \
    "${DEFAULT_FEE_TX_HASH}" \
    "${SCRIPT_SHA256}" \
    "$(printf '%s\n' "${REGISTERED[@]}" | python3 -c 'import json,sys; print(json.dumps([x.strip() for x in sys.stdin if x.strip()]))')" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    out_file,
    chain_id,
    contract_address,
    verifier_address,
    usdc_address,
    treasury,
    legacy_fee,
    default_fee,
    default_fee_tx_hash,
    script_sha256,
    registered_json,
) = sys.argv[1:]

registered = []
for row in json.loads(registered_json):
    chain, register_tx, fee_tx = row.split(":")
    registered.append(
        {
            "chain_id": int(chain),
            "register_tx_hash": register_tx,
            "set_chain_fee_tx_hash": fee_tx,
        }
    )

report = {
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "network": "base_sepolia",
    "script_sha256": script_sha256,
    "chain_id": int(chain_id),
    "contract": {
        "name": "TrustVerifierMultiChain",
        "address": contract_address,
        "verifier": verifier_address,
        "usdc": usdc_address,
        "treasury": treasury,
    },
    "fees": {
        "legacy_fee_wei": int(legacy_fee),
        "default_fee_wei": int(default_fee),
        "set_default_fee_tx_hash": default_fee_tx_hash,
    },
    "registered_chains": registered,
}

with open(out_file, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2)
PY
  echo "  deployment_out=${DEPLOYMENT_OUT}"
fi
