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
OUT_DIR="${OUT_DIR:-${PROJECT_DIR}/artifacts/ops/attestation_phase13_launch/run_${RUN_ID}}"
REPORT_FILE="${OUT_DIR}/phase13_launch_report.json"
VERIFIER_REPORT="${OUT_DIR}/phase13_verifier_report.json"
LOG_FILE="${OUT_DIR}/phase13_launch.log"
PHASE12_DIR="${OUT_DIR}/phase12"
PHASE11_DIR="${OUT_DIR}/phase11"
mkdir -p "${OUT_DIR}"

log() {
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" | tee -a "${LOG_FILE}"
}

status_commands="PASS"
status_phase12="SKIP"
status_phase11="SKIP"
status_phase13_verifier="SKIP"

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    log "missing command: ${cmd}"
    status_commands="FAIL"
  fi
}

for cmd in python3 bash; do
  require_cmd "${cmd}"
done

log "Starting Phase 13 launch gate"
log "mode=${MODE}"
log "out_dir=${OUT_DIR}"

if [[ "${status_commands}" == "PASS" ]]; then
  log "Running Phase 12 readiness gate"
  if MODE="${MODE}" OUT_DIR="${PHASE12_DIR}" "${SCRIPT_DIR}/run_attestation_phase12_external_readiness_gate.sh" >>"${LOG_FILE}" 2>&1; then
    status_phase12="PASS"
  else
    status_phase12="FAIL"
  fi
fi

if [[ "${status_commands}" == "PASS" && "${status_phase12}" == "PASS" ]]; then
  log "Running Phase 11 unified release gate"
  if MODE="${MODE}" OUT_DIR="${PHASE11_DIR}" "${SCRIPT_DIR}/run_attestation_phase11_release_gate.sh" >>"${LOG_FILE}" 2>&1; then
    status_phase11="PASS"
  else
    status_phase11="FAIL"
  fi
fi

if [[ "${status_commands}" == "PASS" && "${status_phase12}" == "PASS" && "${status_phase11}" == "PASS" ]]; then
  log "Running Phase 13 verifier"
  if python3 "${SCRIPT_DIR}/verify_attestation_phase13_launch_gate.py" \
    --run-dir "${OUT_DIR}" \
    --mode "${MODE}" \
    --output "${VERIFIER_REPORT}" >>"${LOG_FILE}" 2>&1; then
    status_phase13_verifier="PASS"
  else
    status_phase13_verifier="FAIL"
  fi
fi

overall="PASS"
if [[ "${status_commands}" != "PASS" || "${status_phase12}" != "PASS" || "${status_phase11}" != "PASS" || "${status_phase13_verifier}" != "PASS" ]]; then
  overall="FAIL"
fi

go_no_go="NO_GO"
if [[ "${MODE}" == "required" && "${overall}" == "PASS" ]]; then
  go_no_go="GO"
fi

python3 - \
  "${REPORT_FILE}" \
  "${RUN_ID}" \
  "${MODE}" \
  "${OUT_DIR}" \
  "${LOG_FILE}" \
  "${VERIFIER_REPORT}" \
  "${overall}" \
  "${go_no_go}" \
  "${status_commands}" \
  "${status_phase12}" \
  "${status_phase11}" \
  "${status_phase13_verifier}" <<'PY'
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
    overall,
    go_no_go,
    status_commands,
    status_phase12,
    status_phase11,
    status_phase13_verifier,
) = sys.argv[1:]

payload = {
    "schema": "nucleusdb/attestation-phase13-launch-report/v1",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "run_id": run_id,
    "mode": mode,
    "out_dir": out_dir,
    "log_file": log_file,
    "checks": {
        "commands": status_commands,
        "phase12_readiness_gate": status_phase12,
        "phase11_release_gate": status_phase11,
        "phase13_verifier": status_phase13_verifier,
    },
    "verifier_report": verifier_report,
    "go_no_go": go_no_go,
    "overall": overall,
}
with open(report_file, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2, sort_keys=True)
    f.write("\n")
print(json.dumps(payload, indent=2, sort_keys=True))
PY

if [[ "${overall}" != "PASS" ]]; then
  log "Phase 13 launch gate FAILED"
  exit 1
fi
if [[ "${go_no_go}" == "GO" ]]; then
  log "Phase 13 launch gate PASSED (GO)"
else
  log "Phase 13 launch gate PASSED (NO_GO in optional mode)"
fi
