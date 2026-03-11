#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
PROJECT_DIR="$(cd -- "${CONTRACTS_DIR}/.." && pwd)"

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

MODE="${MODE:-optional}" # optional|required
case "${MODE}" in
  optional|required) ;;
  *)
    echo "invalid MODE='${MODE}'; expected optional|required" >&2
    exit 1
    ;;
esac

for cmd in python3 openssl; do
  require_cmd "${cmd}"
done

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-${PROJECT_DIR}/artifacts/ops/attestation_phase11_gate/run_${RUN_ID}}"
REPORT_FILE="${OUT_DIR}/phase11_gate_report.json"
VERIFIER_REPORT="${OUT_DIR}/phase11_verifier_report.json"
MUTATION_REPORT="${OUT_DIR}/mutation_fuzz_report.json"
LOG_FILE="${OUT_DIR}/phase11_gate.log"
mkdir -p "${OUT_DIR}"

log() {
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "${LOG_FILE}"
}

log "Starting Phase 11 attestation release gate"
log "mode=${MODE}"
log "out_dir=${OUT_DIR}"

status_rehearsal="SKIP"
status_promotion_optional="SKIP"
status_bundle="SKIP"
status_replay="SKIP"
status_retention="SKIP"
status_mutation_fuzz="FAIL"

if [[ "${MODE}" == "required" ]]; then
  REQ_DIR="${OUT_DIR}/rehearsal_required"
  log "Running required rehearsal chain"
  if OUT_DIR="${REQ_DIR}" "${SCRIPT_DIR}/rehearse_attestation_release_required.sh" >>"${LOG_FILE}" 2>&1; then
    status_rehearsal="PASS"
  else
    status_rehearsal="FAIL"
  fi
else
  OPT_DIR="${OUT_DIR}/promotion_optional"
  log "Running optional promotion chain"
  if BASE_MODE=optional OUT_DIR="${OPT_DIR}" "${SCRIPT_DIR}/promote_attestation_release.sh" >>"${LOG_FILE}" 2>&1; then
    status_promotion_optional="PASS"
  else
    status_promotion_optional="FAIL"
  fi

  if [[ "${status_promotion_optional}" == "PASS" ]]; then
    SIGN_DIR="${OUT_DIR}/ephemeral_signing"
    KEY="${SIGN_DIR}/bundle_signing_key.pem"
    PUB="${SIGN_DIR}/bundle_signing_pub.pem"
    mkdir -p "${SIGN_DIR}"
    openssl genrsa -out "${KEY}" 2048 >>"${LOG_FILE}" 2>&1
    openssl rsa -in "${KEY}" -pubout -out "${PUB}" >>"${LOG_FILE}" 2>&1

    if python3 "${SCRIPT_DIR}/bundle_attestation_evidence.py" \
      --run-dir "${OPT_DIR}" \
      --signing-key "${KEY}" \
      --public-key "${PUB}" \
      --require-signing \
      --output "${OPT_DIR}/evidence_bundle/bundle_report.json" >>"${LOG_FILE}" 2>&1; then
      status_bundle="PASS"
    else
      status_bundle="FAIL"
    fi

    if [[ "${status_bundle}" == "PASS" ]] && python3 "${SCRIPT_DIR}/replay_attestation_promotion_offline.py" \
      --promotion-report "${OPT_DIR}/promotion_report.json" \
      --require-signed-bundle \
      --output "${OPT_DIR}/offline_replay_report.json" >>"${LOG_FILE}" 2>&1; then
      status_replay="PASS"
    else
      status_replay="FAIL"
    fi

    if python3 "${SCRIPT_DIR}/enforce_attestation_bundle_retention.py" \
      --policy "${SCRIPT_DIR}/attestation_evidence_retention_policy_v1.json" \
      --json-report "${OPT_DIR}/retention_report_dry_run.json" >>"${LOG_FILE}" 2>&1; then
      status_retention="PASS"
    else
      status_retention="FAIL"
    fi
  fi
fi

log "Running mutation fuzz suite"
if python3 "${SCRIPT_DIR}/mutation_fuzz_attestation_phase10.py" --contracts-dir "${CONTRACTS_DIR}" --output "${MUTATION_REPORT}" >>"${LOG_FILE}" 2>&1; then
  status_mutation_fuzz="PASS"
else
  status_mutation_fuzz="FAIL"
fi

log "Running Phase 11 verifier"
if python3 "${SCRIPT_DIR}/verify_attestation_phase11_gate.py" --run-dir "${OUT_DIR}" --mode "${MODE}" --output "${VERIFIER_REPORT}" >>"${LOG_FILE}" 2>&1; then
  verifier_ok="PASS"
else
  verifier_ok="FAIL"
fi

overall="PASS"
if [[ "${verifier_ok}" != "PASS" || "${status_mutation_fuzz}" != "PASS" ]]; then
  overall="FAIL"
fi

python3 - \
  "${REPORT_FILE}" \
  "${RUN_ID}" \
  "${MODE}" \
  "${OUT_DIR}" \
  "${LOG_FILE}" \
  "${VERIFIER_REPORT}" \
  "${verifier_ok}" \
  "${overall}" \
  "${status_rehearsal}" \
  "${status_promotion_optional}" \
  "${status_bundle}" \
  "${status_replay}" \
  "${status_retention}" \
  "${status_mutation_fuzz}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    report_file,
    run_id,
    mode,
    out_dir,
    log_file,
    verifier_report,
    verifier_ok,
    overall,
    status_rehearsal,
    status_promotion_optional,
    status_bundle,
    status_replay,
    status_retention,
    status_mutation_fuzz,
) = sys.argv[1:]

payload = {
    "schema": "nucleusdb/attestation-phase11-gate-report/v1",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "run_id": run_id,
    "mode": mode,
    "out_dir": out_dir,
    "log_file": log_file,
    "checks": {
        "rehearsal_required": status_rehearsal,
        "promotion_optional": status_promotion_optional,
        "bundle_signed": status_bundle,
        "offline_replay": status_replay,
        "retention": status_retention,
        "mutation_fuzz": status_mutation_fuzz,
        "phase11_verifier": verifier_ok,
    },
    "verifier_report": verifier_report,
    "overall": overall,
}
with open(report_file, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2, sort_keys=True)
    f.write("\n")
print(json.dumps(payload, indent=2, sort_keys=True))
PY

cat "${REPORT_FILE}"
if [[ "${overall}" != "PASS" ]]; then
  log "Phase 11 gate FAILED"
  exit 1
fi

log "Phase 11 gate PASSED"
