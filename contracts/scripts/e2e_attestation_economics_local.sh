#!/usr/bin/env bash
set -euo pipefail

# End-to-end local economics check:
# deploy mock verifier + mock USDC + TrustVerifier, then attest and confirm fee transfer.
#
# This script never requires a private key. It uses anvil unlocked accounts.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

RPC_URL="${RPC_URL:-http://127.0.0.1:8545}"
AUTO_START_ANVIL="${AUTO_START_ANVIL:-1}"
ANVIL_PORT="${ANVIL_PORT:-8545}"

DEPLOYER="${DEPLOYER:-0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266}"
TREASURY="${TREASURY:-0x70997970C51812dc3A010C7d01b50e0d17dc79C8}"
AGENT="${AGENT:-0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC}"
FEE_WEI="${FEE_WEI:-1000000}"
MINT_AMOUNT="${MINT_AMOUNT:-5000000}"
PROOF_HEX="${PROOF_HEX:-0x01}"
PUBLIC_SIGNALS="${PUBLIC_SIGNALS:-[1,2,3,4,2,1]}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

require_cmd forge
require_cmd cast

ANVIL_PID=""
cleanup() {
  if [[ -n "${ANVIL_PID}" ]]; then
    kill "${ANVIL_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if ! cast chain-id --rpc-url "${RPC_URL}" >/dev/null 2>&1; then
  if [[ "${AUTO_START_ANVIL}" != "1" ]]; then
    echo "RPC unavailable at ${RPC_URL}; set AUTO_START_ANVIL=1 or start anvil manually" >&2
    exit 1
  fi
  require_cmd anvil
  echo "Starting anvil on 127.0.0.1:${ANVIL_PORT} ..."
  anvil --host 127.0.0.1 --port "${ANVIL_PORT}" >/tmp/nucleusdb_anvil.log 2>&1 &
  ANVIL_PID=$!
  sleep 1
  cast chain-id --rpc-url "${RPC_URL}" >/dev/null
fi

cd "${CONTRACTS_DIR}"

extract_deployed() {
  awk '/Deployed to:/{print $3}'
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

echo "Deploying MockTrustProofVerifier ..."
VERIFIER_ADDR=$(forge create \
  --rpc-url "${RPC_URL}" \
  --unlocked \
  --from "${DEPLOYER}" \
  --broadcast \
  mocks/MockTrustProofVerifier.sol:MockTrustProofVerifier | extract_deployed)

if [[ -z "${VERIFIER_ADDR}" ]]; then
  echo "failed to parse verifier deployment address" >&2
  exit 1
fi

echo "Deploying MockUSDC ..."
USDC_ADDR=$(forge create \
  --rpc-url "${RPC_URL}" \
  --unlocked \
  --from "${DEPLOYER}" \
  --broadcast \
  mocks/MockUSDC.sol:MockUSDC | extract_deployed)

if [[ -z "${USDC_ADDR}" ]]; then
  echo "failed to parse USDC deployment address" >&2
  exit 1
fi

echo "Deploying TrustVerifier ..."
TRUST_ADDR=$(forge create \
  --rpc-url "${RPC_URL}" \
  --unlocked \
  --from "${DEPLOYER}" \
  --broadcast \
  TrustVerifier.sol:TrustVerifier \
  --constructor-args "${VERIFIER_ADDR}" "${USDC_ADDR}" "${TREASURY}" "${FEE_WEI}" | extract_deployed)

if [[ -z "${TRUST_ADDR}" ]]; then
  echo "failed to parse TrustVerifier deployment address" >&2
  exit 1
fi

echo "Minting mock USDC to agent ..."
cast send --rpc-url "${RPC_URL}" --unlocked --from "${DEPLOYER}" \
  "${USDC_ADDR}" "mint(address,uint256)" "${AGENT}" "${MINT_AMOUNT}" >/dev/null

echo "Approving TrustVerifier spend ..."
cast send --rpc-url "${RPC_URL}" --unlocked --from "${AGENT}" \
  "${USDC_ADDR}" "approve(address,uint256)" "${TRUST_ADDR}" "${FEE_WEI}" >/dev/null

echo "Attesting and paying fee ..."
cast send --rpc-url "${RPC_URL}" --unlocked --from "${AGENT}" \
  "${TRUST_ADDR}" "attestAndPay(bytes,uint256[])" "${PROOF_HEX}" "${PUBLIC_SIGNALS}" >/dev/null

TREASURY_BAL_RAW=$(cast call --rpc-url "${RPC_URL}" "${USDC_ADDR}" "balanceOf(address)(uint256)" "${TREASURY}")
TREASURY_BAL=$(parse_uint_output "${TREASURY_BAL_RAW}")
VERIFY_AGENT=$(cast call --rpc-url "${RPC_URL}" "${TRUST_ADDR}" "verifyAgent(address)(bool)" "${AGENT}")

if [[ "${TREASURY_BAL}" != "${FEE_WEI}" ]]; then
  echo "treasury fee mismatch: expected ${FEE_WEI}, got ${TREASURY_BAL}" >&2
  exit 1
fi

if [[ "${VERIFY_AGENT}" != "true" ]]; then
  echo "verifyAgent returned ${VERIFY_AGENT}, expected true" >&2
  exit 1
fi

echo "PASS: local economics flow complete"
echo "  verifier=${VERIFIER_ADDR}"
echo "  usdc=${USDC_ADDR}"
echo "  trust=${TRUST_ADDR}"
echo "  treasury_balance=${TREASURY_BAL}"
echo "  verify_agent=${VERIFY_AGENT}"
