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

require_cmd python3

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="${OUT_DIR:-${PROJECT_DIR}/artifacts/ops/attestation_promotion/run_${RUN_ID}}"
mkdir -p "${OUT_DIR}"

# Strict by default for promotion.
BASE_MODE="${BASE_MODE:-required}"
export BASE_MODE

GATE_OUT_DIR="${OUT_DIR}/gate"
VERIFIER_REPORT="${OUT_DIR}/verifier_report.json"
PROMOTION_REPORT="${OUT_DIR}/promotion_report.json"

echo "[promotion] starting"
echo "[promotion] out_dir=${OUT_DIR}"
echo "[promotion] base_mode=${BASE_MODE}"

(
  cd "${CONTRACTS_DIR}"
  OUT_DIR="${GATE_OUT_DIR}" ./scripts/run_attestation_economics_gate.sh
)

VERIFIER_ARGS=(
  "${CONTRACTS_DIR}/scripts/verify_attestation_gate_artifacts.py"
  --run-dir "${GATE_OUT_DIR}"
  --output "${VERIFIER_REPORT}"
)
if [[ "${BASE_MODE}" == "required" ]]; then
  VERIFIER_ARGS+=(--require-base)
fi

python3 "${VERIFIER_ARGS[@]}"

python3 - \
  "${PROMOTION_REPORT}" \
  "${RUN_ID}" \
  "${BASE_MODE}" \
  "${GATE_OUT_DIR}/gate_report.json" \
  "${VERIFIER_REPORT}" <<'PY'
import json
import sys
from datetime import datetime, timezone

promotion_report, run_id, base_mode, gate_report_file, verifier_report_file = sys.argv[1:]
gate = json.loads(open(gate_report_file, "r", encoding="utf-8").read())
verifier = json.loads(open(verifier_report_file, "r", encoding="utf-8").read())
ok = bool(gate.get("overall") == "PASS" and verifier.get("ok") is True)
report = {
    "schema": "nucleusdb/attestation-promotion-report/v1",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "run_id": run_id,
    "base_mode": base_mode,
    "ok": ok,
    "gate_report": gate_report_file,
    "verifier_report": verifier_report_file,
}
with open(promotion_report, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")
print(json.dumps(report, indent=2, sort_keys=True))
if not ok:
    raise SystemExit(1)
PY

echo "[promotion] PASS"
echo "[promotion] promotion_report=${PROMOTION_REPORT}"

