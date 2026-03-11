#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
PROJECT_DIR="$(cd -- "${CONTRACTS_DIR}/.." && pwd)"

MODE="${MODE:-optional}" # optional|required
case "${MODE}" in
  optional|required) ;;
  *)
    echo "invalid MODE='${MODE}'; expected optional|required" >&2
    exit 1
    ;;
esac

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-${PROJECT_DIR}/artifacts/ops/attestation_phase12_readiness/run_${RUN_ID}}"
REPORT_FILE="${OUT_DIR}/phase12_readiness_report.json"
PHASE11_DIR="${OUT_DIR}/phase11_optional"
PHASE11_REPORT="${PHASE11_DIR}/phase11_gate_report.json"
LOG_FILE="${OUT_DIR}/phase12_readiness.log"
mkdir -p "${OUT_DIR}"

log() {
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "${LOG_FILE}"
}

status_commands="PASS"
status_phase11_optional="FAIL"
status_env_preflight="FAIL"
status_signing_material="SKIP"
status_rpc_chain="SKIP"
status_contract_views="SKIP"

missing_env=()
signing_detail="not_checked"
rpc_chain_detail="not_checked"
contract_views_detail="not_checked"

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    log "missing command: ${cmd}"
    status_commands="FAIL"
  fi
}

for cmd in python3 cast openssl; do
  require_cmd "${cmd}"
done

log "Running Phase 12 external readiness gate"
log "mode=${MODE}"
log "out_dir=${OUT_DIR}"

if [[ "${status_commands}" == "PASS" ]]; then
  log "Running Phase 11 optional gate as prerequisite"
  if MODE=optional OUT_DIR="${PHASE11_DIR}" "${SCRIPT_DIR}/run_attestation_phase11_release_gate.sh" >>"${LOG_FILE}" 2>&1; then
    status_phase11_optional="PASS"
  else
    status_phase11_optional="FAIL"
  fi
fi

# Required production env for live required-mode launch.
for v in RPC_URL_BASE_SEPOLIA TRUST_VERIFIER_ADDRESS AGENT_ADDRESS PROOF_HEX PUBLIC_SIGNALS EVIDENCE_SIGNING_KEY_PEM EVIDENCE_SIGNING_PUB_PEM; do
  if [[ -z "${!v:-}" ]]; then
    missing_env+=("${v}")
  fi
done
if [[ -z "${ETH_KEYSTORE:-}" && -z "${PRIVATE_KEY:-}" ]]; then
  missing_env+=("ETH_KEYSTORE|PRIVATE_KEY")
fi
if [[ ${#missing_env[@]} -eq 0 ]]; then
  status_env_preflight="PASS"
else
  status_env_preflight="FAIL"
fi

# Verify signing key material if present.
if [[ -n "${EVIDENCE_SIGNING_KEY_PEM:-}" && -n "${EVIDENCE_SIGNING_PUB_PEM:-}" ]]; then
  if [[ -f "${EVIDENCE_SIGNING_KEY_PEM}" && -f "${EVIDENCE_SIGNING_PUB_PEM}" ]]; then
    tmp_msg="${OUT_DIR}/_signing_probe.txt"
    tmp_sig="${OUT_DIR}/_signing_probe.sig"
    echo "nucleusdb-phase12-signing-probe-${RUN_ID}" >"${tmp_msg}"
    if openssl dgst -sha256 -sign "${EVIDENCE_SIGNING_KEY_PEM}" -out "${tmp_sig}" "${tmp_msg}" >>"${LOG_FILE}" 2>&1 && \
       openssl dgst -sha256 -verify "${EVIDENCE_SIGNING_PUB_PEM}" -signature "${tmp_sig}" "${tmp_msg}" >>"${LOG_FILE}" 2>&1; then
      status_signing_material="PASS"
      signing_detail="sign_verify_ok"
    else
      status_signing_material="FAIL"
      signing_detail="sign_verify_failed"
    fi
  else
    status_signing_material="FAIL"
    signing_detail="signing_files_missing"
  fi
else
  status_signing_material="SKIP"
  signing_detail="signing_env_missing"
fi

# Chain / contract probes if possible.
if [[ -n "${RPC_URL_BASE_SEPOLIA:-}" ]]; then
  set +e
  chain_id_raw="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}" 2>>"${LOG_FILE}")"
  rc=$?
  set -e
  if [[ ${rc} -eq 0 ]]; then
    chain_id="$(echo "${chain_id_raw}" | tr -d '[:space:]')"
    if [[ "${chain_id}" == "84532" ]]; then
      status_rpc_chain="PASS"
      rpc_chain_detail="chain_id_84532"
    else
      status_rpc_chain="FAIL"
      rpc_chain_detail="unexpected_chain_id:${chain_id}"
    fi
  else
    status_rpc_chain="FAIL"
    rpc_chain_detail="rpc_unreachable"
  fi
fi

if [[ -n "${RPC_URL_BASE_SEPOLIA:-}" && -n "${TRUST_VERIFIER_ADDRESS:-}" ]]; then
  set +e
  fee_raw="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "fee()(uint256)" 2>>"${LOG_FILE}")"
  rc_fee=$?
  usdc_raw="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "usdc()(address)" 2>>"${LOG_FILE}")"
  rc_usdc=$?
  treasury_raw="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "treasury()(address)" 2>>"${LOG_FILE}")"
  rc_treasury=$?
  verify_raw=""
  rc_verify=0
  if [[ -n "${AGENT_ADDRESS:-}" ]]; then
    verify_raw="$(cast call --rpc-url "${RPC_URL_BASE_SEPOLIA}" "${TRUST_VERIFIER_ADDRESS}" "verifyAgent(address)(bool)" "${AGENT_ADDRESS}" 2>>"${LOG_FILE}")"
    rc_verify=$?
  fi
  set -e

  if [[ ${rc_fee} -eq 0 && ${rc_usdc} -eq 0 && ${rc_treasury} -eq 0 && ${rc_verify} -eq 0 ]]; then
    status_contract_views="PASS"
    contract_views_detail="fee_usdc_treasury_verify_ok"
  else
    status_contract_views="FAIL"
    contract_views_detail="view_probe_failed"
  fi

  python3 - \
    "${OUT_DIR}/phase12_contract_probe.json" \
    "${fee_raw}" \
    "${usdc_raw}" \
    "${treasury_raw}" \
    "${verify_raw}" <<'PY'
import json
import sys

out_path, fee_raw, usdc_raw, treasury_raw, verify_raw = sys.argv[1:]
payload = {
    "fee_raw": fee_raw,
    "usdc_raw": usdc_raw,
    "treasury_raw": treasury_raw,
    "verify_raw": verify_raw,
}
with open(out_path, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2, sort_keys=True)
    f.write("\n")
PY
fi

overall="PASS"
if [[ "${status_commands}" != "PASS" || "${status_phase11_optional}" != "PASS" ]]; then
  overall="FAIL"
fi
if [[ "${MODE}" == "required" ]]; then
  if [[ "${status_env_preflight}" != "PASS" || "${status_signing_material}" != "PASS" || "${status_rpc_chain}" != "PASS" || "${status_contract_views}" != "PASS" ]]; then
    overall="FAIL"
  fi
fi

python3 - \
  "${REPORT_FILE}" \
  "${RUN_ID}" \
  "${MODE}" \
  "${OUT_DIR}" \
  "${PHASE11_REPORT}" \
  "${LOG_FILE}" \
  "${overall}" \
  "${status_commands}" \
  "${status_phase11_optional}" \
  "${status_env_preflight}" \
  "${status_signing_material}" \
  "${status_rpc_chain}" \
  "${status_contract_views}" \
  "${signing_detail}" \
  "${rpc_chain_detail}" \
  "${contract_views_detail}" \
  "${missing_env[*]:-}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    report_file,
    run_id,
    mode,
    out_dir,
    phase11_report,
    log_file,
    overall,
    status_commands,
    status_phase11_optional,
    status_env_preflight,
    status_signing_material,
    status_rpc_chain,
    status_contract_views,
    signing_detail,
    rpc_chain_detail,
    contract_views_detail,
    missing_env,
) = sys.argv[1:]

missing = [x for x in missing_env.split() if x]
payload = {
    "schema": "nucleusdb/attestation-phase12-external-readiness/v1",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "run_id": run_id,
    "mode": mode,
    "out_dir": out_dir,
    "phase11_report": phase11_report,
    "log_file": log_file,
    "checks": {
        "commands": status_commands,
        "phase11_optional_gate": status_phase11_optional,
        "env_preflight": status_env_preflight,
        "signing_material": status_signing_material,
        "rpc_chain": status_rpc_chain,
        "contract_views": status_contract_views,
    },
    "details": {
        "signing_material": signing_detail,
        "rpc_chain": rpc_chain_detail,
        "contract_views": contract_views_detail,
        "missing_env": missing,
    },
    "overall": overall,
}
with open(report_file, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2, sort_keys=True)
    f.write("\n")
print(json.dumps(payload, indent=2, sort_keys=True))
PY

if [[ "${overall}" != "PASS" ]]; then
  log "Phase 12 readiness FAILED"
  exit 1
fi
log "Phase 12 readiness PASSED"
