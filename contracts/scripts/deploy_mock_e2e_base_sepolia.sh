#!/usr/bin/env bash
set -euo pipefail

# Deploy full mock multichain E2E stack on Base Sepolia:
#   1) MockTrustProofVerifier
#   2) MockUSDC
#   3) TrustVerifierMultiChain wired to mocks
#   4) Mint/approve mock USDC
#   5) Register chains + set tiered fees
#   6) Submit single + multi attestation
#   7) Verify positive/negative views + treasury economics
#   8) Emit JSON evidence
#
# Defaults are tuned for one-command testnet execution.
#
# Env:
#   RPC_URL_BASE_SEPOLIA              (default: https://sepolia.base.org)
#   PRIVATE_KEY                       (required; or set via .env.testnet at repo root)
#   AGENT_PRIVATE_KEY                 (optional; default: PRIVATE_KEY)
#   AGENT_ADDRESS                     (optional; default: address from AGENT_PRIVATE_KEY)
#   TREASURY_ADDRESS                  (optional; default: 0x1111111111111111111111111111111111111111)
#   EVIDENCE_OUT                      (optional JSON path)
#   TX_GAS_PRICE_WEI                  (default: 6000000)
#   MOCK_MINT_AMOUNT                  (default: 100000000)
#   TRUST_DEFAULT_FEE_WEI             (default: 1000000)
#   TRUST_LEGACY_FEE_WEI              (default: 0)
#   TRUST_BASE_CHAIN_FEE_WEI          (default: 1000000)
#   TRUST_ETH_CHAIN_FEE_WEI           (default: 5000000)
#   EXPECT_TREASURY_ABS_BALANCE       (default: 7000000)
#   PROOF_HEX_SINGLE                  (default: 0x01)
#   PROOF_HEX_MULTI                   (default: 0x01)
#   PUBLIC_SIGNALS_SINGLE             (default: [1,2,3,4,2,1])
#   PUBLIC_SIGNALS_MULTI              (default: [1,2,3,4,2,2])
#   CHAINS_SINGLE                     (default: [8453])
#   CHAINS_MULTI                      (default: [8453,1])
#   REQUIRED_MULTI_PASS               (default: [8453,1])
#   REQUIRED_MULTI_FAIL               (default: [8453,42161])

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
# Walk up to find repo root (directory containing Cargo.toml or .git)
_d="${CONTRACTS_DIR}"
while [[ "${_d}" != "/" ]]; do
  if [[ -f "${_d}/Cargo.toml" || -d "${_d}/.git" ]]; then break; fi
  _d="$(dirname "${_d}")"
done
REPO_ROOT="${_d}"
DEFAULT_ENV_FILE="${REPO_ROOT}/.env.testnet"
SCRIPT_SHA256="$(sha256sum "${BASH_SOURCE[0]}" | awk '{print $1}')"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

extract_address() {
  local raw="$1"
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{40}' | head -n1
}

extract_deployed_address() {
  local raw="$1"
  local deployed
  deployed="$(printf '%s\n' "${raw}" | awk '/Deployed to:/{print $3; exit}')"
  if [[ -n "${deployed}" ]]; then
    printf '%s\n' "${deployed}"
    return 0
  fi
  printf '%s\n' "${raw}" | grep -Eo '0x[0-9a-fA-F]{40}' | tail -n1
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

load_defaults_from_env_file() {
  if [[ -z "${PRIVATE_KEY:-}" && -f "${DEFAULT_ENV_FILE}" ]]; then
    # shellcheck disable=SC1090
    set -a
    source "${DEFAULT_ENV_FILE}"
    set +a
    if [[ -n "${RPC_URL:-}" && -z "${RPC_URL_BASE_SEPOLIA:-}" ]]; then
      RPC_URL_BASE_SEPOLIA="${RPC_URL}"
    fi
  fi
}

run_send_with_retry() {
  local sender_addr="$1"
  local signer_key="$2"
  local contract="$3"
  local selector="$4"
  shift 4

  local base_gas="${TX_GAS_PRICE_WEI}"
  local gas_prices=("${base_gas}" "12000000" "24000000" "60000000")
  local out=""

  for gas in "${gas_prices[@]}"; do
    local nonce
    nonce="$(cast nonce --rpc-url "${RPC_URL_BASE_SEPOLIA}" --block pending "${sender_addr}" | tr -d '[:space:]')"
    set +e
    out="$(
      cast send \
        --rpc-url "${RPC_URL_BASE_SEPOLIA}" \
        --gas-price "${gas}" \
        --nonce "${nonce}" \
        --private-key "${signer_key}" \
        "${contract}" \
        "${selector}" \
        "$@" 2>&1
    )"
    local rc=$?
    set -e

    if [[ ${rc} -eq 0 ]]; then
      printf '%s\n' "${out}"
      return 0
    fi

    if printf '%s' "${out}" | grep -Eiq 'replacement transaction underpriced|nonce too low'; then
      sleep 2
      continue
    fi

    printf '%s\n' "${out}" >&2
    return "${rc}"
  done

  printf '%s\n' "${out}" >&2
  return 1
}

deploy_with_retry() {
  local deployer_addr="$1"
  local signer_key="$2"
  local contract_ref="$3"
  shift 3

  local base_gas="${TX_GAS_PRICE_WEI}"
  local gas_prices=("${base_gas}" "12000000" "24000000" "60000000")
  local out=""

  for gas in "${gas_prices[@]}"; do
    local nonce
    nonce="$(cast nonce --rpc-url "${RPC_URL_BASE_SEPOLIA}" --block pending "${deployer_addr}" | tr -d '[:space:]')"
    local cmd=(
      forge create
      --rpc-url "${RPC_URL_BASE_SEPOLIA}"
      --broadcast
      --gas-price "${gas}"
      --nonce "${nonce}"
      --private-key "${signer_key}"
      "${contract_ref}"
    )
    if [[ $# -gt 0 ]]; then
      cmd+=(--constructor-args "$@")
    fi
    set +e
    out="$("${cmd[@]}" 2>&1)"
    local rc=$?
    set -e

    if [[ ${rc} -eq 0 ]]; then
      printf '%s\n' "${out}"
      return 0
    fi

    if printf '%s' "${out}" | grep -Eiq 'replacement transaction underpriced|nonce too low'; then
      sleep 2
      continue
    fi

    printf '%s\n' "${out}" >&2
    return "${rc}"
  done

  printf '%s\n' "${out}" >&2
  return 1
}

register_and_fee() {
  local contract="$1"
  local chain_id="$2"
  local verifier_addr="$3"
  local fee="$4"

  local metadata_hash
  metadata_hash="$(cast keccak "nucleusdb.multichain.registry.v1|${chain_id}|${verifier_addr}")"

  local reg_out reg_tx fee_out fee_tx
  reg_out="$(run_send_with_retry "${DEPLOYER_ADDRESS}" "${PRIVATE_KEY}" "${contract}" "registerChain(uint256,address,bytes32)" "${chain_id}" "${verifier_addr}" "${metadata_hash}")"
  reg_tx="$(extract_tx_hash "${reg_out}")"
  if [[ -z "${reg_tx}" ]]; then
    echo "failed to parse registerChain tx hash for chain ${chain_id}" >&2
    exit 1
  fi

  fee_out="$(run_send_with_retry "${DEPLOYER_ADDRESS}" "${PRIVATE_KEY}" "${contract}" "setChainFee(uint256,uint256)" "${chain_id}" "${fee}")"
  fee_tx="$(extract_tx_hash "${fee_out}")"
  if [[ -z "${fee_tx}" ]]; then
    echo "failed to parse setChainFee tx hash for chain ${chain_id}" >&2
    exit 1
  fi

  local registered_ok observed_fee
  registered_ok=""
  observed_fee=""
  for _attempt in 1 2 3 4; do
    local chain_info
    chain_info="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${contract}" "chainInfo(uint256)(bool,address,bytes32,uint64,uint256)" "${chain_id}")"
    registered_ok="$(normalize_bool_output "$(printf '%s\n' "${chain_info}" | sed -n '1p')")"
    observed_fee="$(parse_uint_output "$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${contract}" "chainFees(uint256)(uint256)" "${chain_id}")")"
    if [[ "${registered_ok}" == "true" && "${observed_fee}" == "${fee}" ]]; then
      break
    fi
    sleep 2
  done

  if [[ "${registered_ok}" != "true" ]]; then
    echo "postcondition failed: chain ${chain_id} not registered" >&2
    exit 1
  fi
  if [[ "${observed_fee}" != "${fee}" ]]; then
    echo "postcondition failed: chain ${chain_id} fee expected ${fee}, got ${observed_fee}" >&2
    exit 1
  fi

  echo "${chain_id}:${reg_tx}:${fee_tx}"
}

require_cmd forge
require_cmd cast
require_cmd python3

load_defaults_from_env_file

RPC_URL_BASE_SEPOLIA="${RPC_URL_BASE_SEPOLIA:-https://sepolia.base.org}"
TX_GAS_PRICE_WEI="${TX_GAS_PRICE_WEI:-6000000}"
MOCK_MINT_AMOUNT="${MOCK_MINT_AMOUNT:-100000000}"
TRUST_LEGACY_FEE_WEI="${TRUST_LEGACY_FEE_WEI:-0}"
TRUST_DEFAULT_FEE_WEI="${TRUST_DEFAULT_FEE_WEI:-1000000}"
TRUST_BASE_CHAIN_FEE_WEI="${TRUST_BASE_CHAIN_FEE_WEI:-1000000}"
TRUST_ETH_CHAIN_FEE_WEI="${TRUST_ETH_CHAIN_FEE_WEI:-5000000}"
EXPECT_TREASURY_ABS_BALANCE="${EXPECT_TREASURY_ABS_BALANCE:-7000000}"

PROOF_HEX_SINGLE="${PROOF_HEX_SINGLE:-0x01}"
PROOF_HEX_MULTI="${PROOF_HEX_MULTI:-0x01}"
PUBLIC_SIGNALS_SINGLE="${PUBLIC_SIGNALS_SINGLE:-[1,2,3,4,2,1]}"
PUBLIC_SIGNALS_MULTI="${PUBLIC_SIGNALS_MULTI:-[1,2,3,4,2,2]}"
CHAINS_SINGLE="${CHAINS_SINGLE:-[8453]}"
CHAINS_MULTI="${CHAINS_MULTI:-[8453,1]}"
REQUIRED_MULTI_PASS="${REQUIRED_MULTI_PASS:-[8453,1]}"
REQUIRED_MULTI_FAIL="${REQUIRED_MULTI_FAIL:-[8453,42161]}"

if [[ -z "${PRIVATE_KEY:-}" ]]; then
  echo "missing PRIVATE_KEY (and no usable default in ${DEFAULT_ENV_FILE})" >&2
  exit 1
fi

AGENT_PRIVATE_KEY="${AGENT_PRIVATE_KEY:-${PRIVATE_KEY}}"
DEPLOYER_ADDRESS="$(cast wallet address --private-key "${PRIVATE_KEY}" | tr -d '[:space:]')"
AGENT_ADDRESS="${AGENT_ADDRESS:-$(cast wallet address --private-key "${AGENT_PRIVATE_KEY}" | tr -d '[:space:]')}"
TREASURY_ADDRESS="${TREASURY_ADDRESS:-0x1111111111111111111111111111111111111111}"

AGENT_FROM_KEY="$(cast wallet address --private-key "${AGENT_PRIVATE_KEY}" | tr -d '[:space:]')"
if [[ "${AGENT_FROM_KEY,,}" != "${AGENT_ADDRESS,,}" ]]; then
  echo "AGENT_PRIVATE_KEY does not match AGENT_ADDRESS" >&2
  exit 1
fi

CHAIN_ID="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}")"
if [[ "${CHAIN_ID}" != "84532" ]]; then
  echo "unexpected chain id: ${CHAIN_ID} (expected 84532 Base Sepolia)" >&2
  exit 1
fi

cd "${CONTRACTS_DIR}"

echo "Deploying MockTrustProofVerifier..."
VERIFIER_OUT="$(deploy_with_retry "${DEPLOYER_ADDRESS}" "${PRIVATE_KEY}" "mocks/MockTrustProofVerifier.sol:MockTrustProofVerifier")"
MOCK_VERIFIER_ADDRESS="$(extract_deployed_address "${VERIFIER_OUT}")"
MOCK_VERIFIER_DEPLOY_TX_HASH="$(extract_tx_hash "${VERIFIER_OUT}")"
if [[ -z "${MOCK_VERIFIER_ADDRESS}" || -z "${MOCK_VERIFIER_DEPLOY_TX_HASH}" ]]; then
  echo "failed to parse mock verifier deployment output" >&2
  exit 1
fi

echo "Deploying MockUSDC..."
USDC_OUT="$(deploy_with_retry "${DEPLOYER_ADDRESS}" "${PRIVATE_KEY}" "mocks/MockUSDC.sol:MockUSDC")"
MOCK_USDC_ADDRESS="$(extract_deployed_address "${USDC_OUT}")"
MOCK_USDC_DEPLOY_TX_HASH="$(extract_tx_hash "${USDC_OUT}")"
if [[ -z "${MOCK_USDC_ADDRESS}" || -z "${MOCK_USDC_DEPLOY_TX_HASH}" ]]; then
  echo "failed to parse mock USDC deployment output" >&2
  exit 1
fi

echo "Minting mock USDC..."
MINT_OUT="$(run_send_with_retry "${DEPLOYER_ADDRESS}" "${PRIVATE_KEY}" "${MOCK_USDC_ADDRESS}" "mint(address,uint256)" "${AGENT_ADDRESS}" "${MOCK_MINT_AMOUNT}")"
MINT_TX_HASH="$(extract_tx_hash "${MINT_OUT}")"
if [[ -z "${MINT_TX_HASH}" ]]; then
  echo "failed to parse mock mint tx hash" >&2
  exit 1
fi

echo "Deploying TrustVerifierMultiChain wired to mocks..."
MULTI_OUT="$(deploy_with_retry "${DEPLOYER_ADDRESS}" "${PRIVATE_KEY}" "TrustVerifierMultiChain.sol:TrustVerifierMultiChain" "${MOCK_VERIFIER_ADDRESS}" "${MOCK_USDC_ADDRESS}" "${TREASURY_ADDRESS}" "${TRUST_LEGACY_FEE_WEI}" "${TRUST_DEFAULT_FEE_WEI}")"
TRUST_VERIFIER_MULTI_CHAIN_ADDRESS="$(extract_deployed_address "${MULTI_OUT}")"
TRUST_VERIFIER_MULTI_CHAIN_DEPLOY_TX_HASH="$(extract_tx_hash "${MULTI_OUT}")"
if [[ -z "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" || -z "${TRUST_VERIFIER_MULTI_CHAIN_DEPLOY_TX_HASH}" ]]; then
  echo "failed to parse TrustVerifierMultiChain deployment output" >&2
  exit 1
fi

echo "Registering chains and setting tiered fees..."
REGISTER_8453="$(register_and_fee "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "8453" "${MOCK_VERIFIER_ADDRESS}" "${TRUST_BASE_CHAIN_FEE_WEI}")"
REGISTER_1="$(register_and_fee "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "1" "${MOCK_VERIFIER_ADDRESS}" "${TRUST_ETH_CHAIN_FEE_WEI}")"

IFS=':' read -r _CHAIN8453 REG_8453_TX SET_FEE_8453_TX <<< "${REGISTER_8453}"
IFS=':' read -r _CHAIN1 REG_1_TX SET_FEE_1_TX <<< "${REGISTER_1}"

echo "Approving mock USDC spend..."
APPROVE_AMOUNT="50000000"
APPROVE_OUT="$(run_send_with_retry "${AGENT_ADDRESS}" "${AGENT_PRIVATE_KEY}" "${MOCK_USDC_ADDRESS}" "approve(address,uint256)" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "${APPROVE_AMOUNT}")"
APPROVE_TX_HASH="$(extract_tx_hash "${APPROVE_OUT}")"
if [[ -z "${APPROVE_TX_HASH}" ]]; then
  echo "failed to parse approve tx hash" >&2
  exit 1
fi

PRE_TREASURY_RAW="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${MOCK_USDC_ADDRESS}" "balanceOf(address)(uint256)" "${TREASURY_ADDRESS}")"
PRE_TREASURY_BALANCE="$(parse_uint_output "${PRE_TREASURY_RAW}")"

echo "Submitting single-chain attestation..."
SINGLE_OUT="$(run_send_with_retry "${AGENT_ADDRESS}" "${AGENT_PRIVATE_KEY}" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "submitCompositeAttestation(bytes,uint256[],uint256[])" "${PROOF_HEX_SINGLE}" "${PUBLIC_SIGNALS_SINGLE}" "${CHAINS_SINGLE}")"
SINGLE_TX_HASH="$(extract_tx_hash "${SINGLE_OUT}")"
if [[ -z "${SINGLE_TX_HASH}" ]]; then
  echo "failed to parse single attestation tx hash" >&2
  exit 1
fi
SINGLE_RECEIPT_STATUS="$(receipt_status "${SINGLE_TX_HASH}")"
if [[ "${SINGLE_RECEIPT_STATUS}" != "1" ]]; then
  echo "single attestation receipt failed: ${SINGLE_RECEIPT_STATUS}" >&2
  exit 1
fi

echo "Submitting multi-chain attestation..."
MULTI_ATTEST_OUT="$(run_send_with_retry "${AGENT_ADDRESS}" "${AGENT_PRIVATE_KEY}" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "submitCompositeAttestation(bytes,uint256[],uint256[])" "${PROOF_HEX_MULTI}" "${PUBLIC_SIGNALS_MULTI}" "${CHAINS_MULTI}")"
MULTI_ATTEST_TX_HASH="$(extract_tx_hash "${MULTI_ATTEST_OUT}")"
if [[ -z "${MULTI_ATTEST_TX_HASH}" ]]; then
  echo "failed to parse multi attestation tx hash" >&2
  exit 1
fi
MULTI_RECEIPT_STATUS="$(receipt_status "${MULTI_ATTEST_TX_HASH}")"
if [[ "${MULTI_RECEIPT_STATUS}" != "1" ]]; then
  echo "multi attestation receipt failed: ${MULTI_RECEIPT_STATUS}" >&2
  exit 1
fi

VERIFY_8453=""
VERIFY_1=""
VERIFY_MULTI_PASS=""
VERIFY_MULTI_FAIL=""
for _attempt in 1 2 3 4; do
  VERIFY_8453="$(normalize_bool_output "$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "isVerifiedForChain(address,uint256)(bool)" "${AGENT_ADDRESS}" "8453")")"
  VERIFY_1="$(normalize_bool_output "$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "isVerifiedForChain(address,uint256)(bool)" "${AGENT_ADDRESS}" "1")")"
  VERIFY_MULTI_PASS="$(normalize_bool_output "$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "isVerifiedMultiChain(address,uint256[])(bool)" "${AGENT_ADDRESS}" "${REQUIRED_MULTI_PASS}")")"
  VERIFY_MULTI_FAIL="$(normalize_bool_output "$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" "isVerifiedMultiChain(address,uint256[])(bool)" "${AGENT_ADDRESS}" "${REQUIRED_MULTI_FAIL}")")"
  if [[ "${VERIFY_8453}" == "true" && "${VERIFY_1}" == "true" && "${VERIFY_MULTI_PASS}" == "true" && "${VERIFY_MULTI_FAIL}" == "false" ]]; then
    break
  fi
  sleep 2
done

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

POST_TREASURY_RAW="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${MOCK_USDC_ADDRESS}" "balanceOf(address)(uint256)" "${TREASURY_ADDRESS}")"
POST_TREASURY_BALANCE="$(parse_uint_output "${POST_TREASURY_RAW}")"
TREASURY_DELTA=$((POST_TREASURY_BALANCE - PRE_TREASURY_BALANCE))
# single@[8453] pays BASE; multi@[8453,1] pays BASE+ETH; total = 2*BASE + ETH
EXPECTED_DELTA=$((TRUST_BASE_CHAIN_FEE_WEI + TRUST_BASE_CHAIN_FEE_WEI + TRUST_ETH_CHAIN_FEE_WEI))

if [[ "${TREASURY_DELTA}" != "${EXPECTED_DELTA}" ]]; then
  echo "treasury delta mismatch: expected ${EXPECTED_DELTA}, got ${TREASURY_DELTA}" >&2
  exit 1
fi
if [[ "${POST_TREASURY_BALANCE}" != "${EXPECT_TREASURY_ABS_BALANCE}" ]]; then
  echo "treasury absolute mismatch: expected ${EXPECT_TREASURY_ABS_BALANCE}, got ${POST_TREASURY_BALANCE}" >&2
  exit 1
fi

if [[ -z "${EVIDENCE_OUT:-}" ]]; then
  EVIDENCE_OUT="artifacts/ops/multichain_sepolia/mock_e2e_$(date -u +%Y%m%dT%H%M%SZ).json"
fi
mkdir -p "$(dirname "${EVIDENCE_OUT}")"

python3 - \
  "${EVIDENCE_OUT}" \
  "${CHAIN_ID}" \
  "${SCRIPT_SHA256}" \
  "${RPC_URL_BASE_SEPOLIA}" \
  "${TX_GAS_PRICE_WEI}" \
  "${MOCK_MINT_AMOUNT}" \
  "${TRUST_DEFAULT_FEE_WEI}" \
  "${TRUST_BASE_CHAIN_FEE_WEI}" \
  "${TRUST_ETH_CHAIN_FEE_WEI}" \
  "${DEPLOYER_ADDRESS}" \
  "${AGENT_ADDRESS}" \
  "${TREASURY_ADDRESS}" \
  "${MOCK_VERIFIER_ADDRESS}" \
  "${MOCK_USDC_ADDRESS}" \
  "${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}" \
  "${MOCK_VERIFIER_DEPLOY_TX_HASH}" \
  "${MOCK_USDC_DEPLOY_TX_HASH}" \
  "${MINT_TX_HASH}" \
  "${TRUST_VERIFIER_MULTI_CHAIN_DEPLOY_TX_HASH}" \
  "${REG_8453_TX}" \
  "${SET_FEE_8453_TX}" \
  "${REG_1_TX}" \
  "${SET_FEE_1_TX}" \
  "${APPROVE_TX_HASH}" \
  "${SINGLE_TX_HASH}" \
  "${MULTI_ATTEST_TX_HASH}" \
  "${SINGLE_RECEIPT_STATUS}" \
  "${MULTI_RECEIPT_STATUS}" \
  "${VERIFY_8453}" \
  "${VERIFY_1}" \
  "${VERIFY_MULTI_PASS}" \
  "${VERIFY_MULTI_FAIL}" \
  "${PRE_TREASURY_BALANCE}" \
  "${POST_TREASURY_BALANCE}" \
  "${TREASURY_DELTA}" \
  "${EXPECTED_DELTA}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
  out_file,
  chain_id,
  script_sha256,
  rpc_url,
  tx_gas_price_wei,
  mock_mint_amount,
  trust_default_fee_wei,
  trust_base_chain_fee_wei,
  trust_eth_chain_fee_wei,
  deployer,
  agent,
  treasury,
  mock_verifier,
  mock_usdc,
  trust_verifier_multichain,
  deploy_mock_verifier_tx,
  deploy_mock_usdc_tx,
  mint_mock_usdc_tx,
  deploy_multichain_tx,
  register_chain_8453_tx,
  set_chain_fee_8453_tx,
  register_chain_1_tx,
  set_chain_fee_1_tx,
  approve_tx,
  single_attestation_tx,
  multi_attestation_tx,
  single_receipt_status,
  multi_receipt_status,
  verify_8453,
  verify_1,
  verify_multi_pass,
  verify_multi_fail,
  pre_treasury_balance,
  post_treasury_balance,
  treasury_delta,
  expected_delta,
) = sys.argv[1:]

def b(x: str) -> bool:
    return x.strip().lower() in ("true", "1", "0x1")

report = {
  "timestamp_utc": datetime.now(timezone.utc).isoformat(),
  "network": "base_sepolia",
  "chain_id": int(chain_id),
  "script_sha256": script_sha256,
  "config": {
    "rpc_url": rpc_url,
    "tx_gas_price_wei": int(tx_gas_price_wei),
    "mock_mint_amount": int(mock_mint_amount),
    "trust_default_fee_wei": int(trust_default_fee_wei),
    "trust_base_chain_fee_wei": int(trust_base_chain_fee_wei),
    "trust_eth_chain_fee_wei": int(trust_eth_chain_fee_wei),
  },
  "actors": {
    "deployer": deployer,
    "agent": agent,
    "treasury": treasury,
  },
  "contracts": {
    "mock_verifier": mock_verifier,
    "mock_usdc": mock_usdc,
    "trust_verifier_multichain": trust_verifier_multichain,
  },
  "tx_hashes": {
    "deploy_mock_verifier": deploy_mock_verifier_tx,
    "deploy_mock_usdc": deploy_mock_usdc_tx,
    "mint_mock_usdc": mint_mock_usdc_tx,
    "deploy_multichain": deploy_multichain_tx,
    "register_chain_8453": register_chain_8453_tx,
    "set_chain_fee_8453": set_chain_fee_8453_tx,
    "register_chain_1": register_chain_1_tx,
    "set_chain_fee_1": set_chain_fee_1_tx,
    "approve": approve_tx,
    "single_attestation": single_attestation_tx,
    "multi_attestation": multi_attestation_tx,
  },
  "receipts": {
    "single_status": int(single_receipt_status),
    "multi_status": int(multi_receipt_status),
  },
  "verification": {
    "is_verified_for_chain_8453": b(verify_8453),
    "is_verified_for_chain_1": b(verify_1),
    "is_verified_multichain_pass": b(verify_multi_pass),
    "is_verified_multichain_fail_expected_false": (not b(verify_multi_fail)),
  },
  "economics": {
    "pre_treasury_balance": int(pre_treasury_balance),
    "post_treasury_balance": int(post_treasury_balance),
    "treasury_delta": int(treasury_delta),
    "expected_delta": int(expected_delta),
  },
}

with open(out_file, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2)
PY

echo "PASS: mock multichain Base Sepolia E2E complete"
echo "  mock_verifier=${MOCK_VERIFIER_ADDRESS}"
echo "  mock_usdc=${MOCK_USDC_ADDRESS}"
echo "  trust_verifier_multichain=${TRUST_VERIFIER_MULTI_CHAIN_ADDRESS}"
echo "  treasury_delta=${TREASURY_DELTA}"
echo "  evidence_out=${EVIDENCE_OUT}"
