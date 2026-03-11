#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
PROJECT_DIR="$(cd "${CONTRACTS_DIR}/.." && pwd)"

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-${PROJECT_DIR}/artifacts/ops/attestation_economics_gate/run_${RUN_ID}}"
mkdir -p "${OUT_DIR}"
BASE_MODE="${BASE_MODE:-optional}"

LOG_FILE="${OUT_DIR}/gate.log"
REPORT_FILE="${OUT_DIR}/gate_report.json"

log() {
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "${LOG_FILE}"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    return 1
  fi
  return 0
}

cmd_ok() {
  set +e
  "$@" >>"${LOG_FILE}" 2>&1
  local rc=$?
  set -e
  return $rc
}

status_private_key_response_check="FAIL"
status_forge_test="FAIL"
status_local_e2e="FAIL"
status_base_preflight="SKIP"
status_base_e2e="SKIP"
base_reason="not requested"
base_evidence_file=""

log "Starting attestation economics gate"
log "OUT_DIR=${OUT_DIR}"
log "BASE_MODE=${BASE_MODE}"

if [[ "${BASE_MODE}" != "optional" && "${BASE_MODE}" != "required" && "${BASE_MODE}" != "skip" ]]; then
  log "invalid BASE_MODE: ${BASE_MODE} (expected optional|required|skip)"
  BASE_MODE="required"
fi

if ! require_cmd rg || ! require_cmd python3 || ! require_cmd forge || ! require_cmd cast; then
  base_reason="missing_required_commands"
  overall="FAIL"
else
  log "Checking MCP response structs for forbidden key material fields"
  if rg -n "pub struct Agent(Register|Verify|Reattest)Response" "${PROJECT_DIR}/src/mcp/tools.rs" -A 40 | rg -q "private_key|keystore_password_file"; then
    status_private_key_response_check="FAIL"
    log "Forbidden key field detected in response struct"
  else
    status_private_key_response_check="PASS"
    log "Response struct check passed"
  fi

  log "Running forge test"
  if cmd_ok bash -lc "cd '${CONTRACTS_DIR}' && forge test"; then
    status_forge_test="PASS"
    log "forge test PASS"
  else
    status_forge_test="FAIL"
    log "forge test FAIL"
  fi

  log "Running local economics E2E"
  if cmd_ok bash -lc "cd '${CONTRACTS_DIR}' && ./scripts/e2e_attestation_economics_local.sh"; then
    status_local_e2e="PASS"
    log "local economics PASS"
  else
    status_local_e2e="FAIL"
    log "local economics FAIL"
  fi

  base_required=(
    RPC_URL_BASE_SEPOLIA
    TRUST_VERIFIER_ADDRESS
    AGENT_ADDRESS
    PROOF_HEX
    PUBLIC_SIGNALS
  )
  missing=()
  for v in "${base_required[@]}"; do
    if [[ -z "${!v:-}" ]]; then
      missing+=("$v")
    fi
  done

  if [[ "${BASE_MODE}" == "skip" ]]; then
    status_base_preflight="SKIP"
    status_base_e2e="SKIP"
    base_reason="forced_skip"
    log "Base Sepolia run skipped: ${base_reason}"
  elif [[ ${#missing[@]} -gt 0 ]]; then
    if [[ "${BASE_MODE}" == "required" ]]; then
      status_base_preflight="FAIL"
      status_base_e2e="SKIP"
      base_reason="missing_env_required:${missing[*]}"
      log "Base Sepolia preflight FAIL: ${base_reason}"
    else
      status_base_preflight="SKIP"
      status_base_e2e="SKIP"
      base_reason="missing_env:${missing[*]}"
      log "Base Sepolia run skipped: ${base_reason}"
    fi
  elif [[ -z "${ETH_KEYSTORE:-}" && -z "${PRIVATE_KEY:-}" ]]; then
    if [[ "${BASE_MODE}" == "required" ]]; then
      status_base_preflight="FAIL"
      status_base_e2e="SKIP"
      base_reason="missing_signer_credentials_required"
      log "Base Sepolia preflight FAIL: ${base_reason}"
    else
      status_base_preflight="SKIP"
      status_base_e2e="SKIP"
      base_reason="missing_signer_credentials"
      log "Base Sepolia run skipped: ${base_reason}"
    fi
  else
    status_base_preflight="PASS"
    chain_id="unknown"
    if cmd_ok cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}"; then
      chain_id="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}" 2>>"${LOG_FILE}" || echo unknown)"
      if [[ "${chain_id}" != "84532" ]]; then
        status_base_preflight="FAIL"
        status_base_e2e="SKIP"
        base_reason="wrong_chain_id:${chain_id}"
        log "Base preflight FAIL: expected 84532 got ${chain_id}"
      fi
    else
      status_base_preflight="FAIL"
      status_base_e2e="SKIP"
      base_reason="rpc_unreachable"
      log "Base preflight FAIL: rpc unreachable"
    fi

    if [[ "${status_base_preflight}" == "PASS" ]]; then
      log "Running Base Sepolia economics E2E"
      base_evidence_file="${OUT_DIR}/base_e2e_evidence.json"
      if cmd_ok env EVIDENCE_OUT="${base_evidence_file}" bash -lc "cd '${CONTRACTS_DIR}' && ./scripts/e2e_attestation_economics_base_sepolia.sh"; then
        status_base_e2e="PASS"
        base_reason="executed"
        log "Base economics PASS"
      else
        status_base_e2e="FAIL"
        base_reason="execution_failed"
        log "Base economics FAIL"
      fi
    fi
  fi

  overall="PASS"
  if [[ "${status_private_key_response_check}" != "PASS" || "${status_forge_test}" != "PASS" || "${status_local_e2e}" != "PASS" ]]; then
    overall="FAIL"
  fi
  if [[ "${status_base_preflight}" == "FAIL" || "${status_base_e2e}" == "FAIL" ]]; then
    overall="FAIL"
  fi
fi

commit_ref="$(git -C "${PROJECT_DIR}" rev-parse --short HEAD 2>/dev/null || echo unknown)"

python3 - \
  "${REPORT_FILE}" \
  "${commit_ref}" \
  "${status_private_key_response_check}" \
  "${status_forge_test}" \
  "${status_local_e2e}" \
  "${status_base_preflight}" \
  "${status_base_e2e}" \
  "${base_reason}" \
  "${overall}" \
  "${LOG_FILE}" \
  "${BASE_MODE}" \
  "${base_evidence_file}" <<'PY'
import sys
import json
from datetime import datetime, timezone
(
    report_file,
    commit_ref,
    status_private_key_response_check,
    status_forge_test,
    status_local_e2e,
    status_base_preflight,
    status_base_e2e,
    base_reason,
    overall,
    log_file,
    base_mode,
    base_evidence_file,
) = sys.argv[1:]
report = {
  "timestamp_utc": datetime.now(timezone.utc).isoformat(),
  "commit": commit_ref,
  "gate": "attestation_economics",
  "checks": {
    "private_key_response_check": status_private_key_response_check,
    "forge_test": status_forge_test,
    "local_e2e": status_local_e2e,
    "base_preflight": status_base_preflight,
    "base_e2e": status_base_e2e
  },
  "base_reason": base_reason,
  "base_mode": base_mode,
  "overall": overall,
  "log_file": log_file,
  "base_evidence_file": base_evidence_file or None,
}
with open(report_file, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2)
PY

log "Wrote report: ${REPORT_FILE}"
python3 -m json.tool "${REPORT_FILE}"

if [[ "${overall}" != "PASS" ]]; then
  log "Gate failed"
  exit 1
fi

log "Gate passed"
