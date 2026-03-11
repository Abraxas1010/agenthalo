#!/usr/bin/env bash
set -euo pipefail

# Deploy NucleusDB TrustVerifier on Base Sepolia.
#
# Required env:
#   RPC_URL_BASE_SEPOLIA
#   TRUST_VERIFIER_GROTH16_VERIFIER
#   TRUST_TREASURY
#
# Optional env:
#   USDC_BASE_SEPOLIA   (default: Base Sepolia USDC)
#   TRUST_FEE_WEI       (default: 0)
#   PRIVATE_KEY         (legacy raw key path; prefer keystore)
#   ETH_KEYSTORE        (preferred signer path)
#   ETH_PASSWORD_FILE   (password file path for ETH_KEYSTORE; ETH_PASSWORD accepted as fallback)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "missing required env: ${name}" >&2
    exit 1
  fi
}

require_env "RPC_URL_BASE_SEPOLIA"
require_env "TRUST_VERIFIER_GROTH16_VERIFIER"
require_env "TRUST_TREASURY"

USDC_BASE_SEPOLIA="${USDC_BASE_SEPOLIA:-0x036CbD53842C5426634e7929541eC2318f3dCF7e}"
TRUST_FEE_WEI="${TRUST_FEE_WEI:-0}"

echo "Deploying TrustVerifier to Base Sepolia..."
echo "verifier=${TRUST_VERIFIER_GROTH16_VERIFIER}"
echo "usdc=${USDC_BASE_SEPOLIA}"
echo "treasury=${TRUST_TREASURY}"
echo "feeWei=${TRUST_FEE_WEI}"

cd "${CONTRACTS_DIR}"

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

forge create \
  --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
  "${SIGNER_ARGS[@]}" \
  TrustVerifier.sol:TrustVerifier \
  --constructor-args \
    "${TRUST_VERIFIER_GROTH16_VERIFIER}" \
    "${USDC_BASE_SEPOLIA}" \
    "${TRUST_TREASURY}" \
    "${TRUST_FEE_WEI}"
