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

require_env() {
  local v="$1"
  if [[ -z "${!v:-}" ]]; then
    echo "missing required env: ${v}" >&2
    exit 1
  fi
}

require_cmd cast
require_cmd python3
require_cmd openssl

# Required Base Sepolia + signer inputs.
# USDC_BASE_SEPOLIA and TRUST_TREASURY can be auto-derived from TrustVerifier by
# e2e_attestation_economics_base_sepolia.sh, so they are optional here.
for v in RPC_URL_BASE_SEPOLIA TRUST_VERIFIER_ADDRESS AGENT_ADDRESS PROOF_HEX PUBLIC_SIGNALS; do
  require_env "${v}"
done
if [[ -z "${ETH_KEYSTORE:-}" && -z "${PRIVATE_KEY:-}" ]]; then
  echo "missing signer credentials: set ETH_KEYSTORE (preferred) or PRIVATE_KEY" >&2
  exit 1
fi

# Required evidence signing inputs.
require_env EVIDENCE_SIGNING_KEY_PEM
require_env EVIDENCE_SIGNING_PUB_PEM
[[ -f "${EVIDENCE_SIGNING_KEY_PEM}" ]] || { echo "signing key file missing: ${EVIDENCE_SIGNING_KEY_PEM}" >&2; exit 1; }
[[ -f "${EVIDENCE_SIGNING_PUB_PEM}" ]] || { echo "public key file missing: ${EVIDENCE_SIGNING_PUB_PEM}" >&2; exit 1; }

CHAIN_ID="$(cast chain-id --rpc-url "${RPC_URL_BASE_SEPOLIA}" | tr -d '[:space:]')"
if [[ "${CHAIN_ID}" != "84532" ]]; then
  echo "wrong chain id for rehearsal: expected 84532, got ${CHAIN_ID}" >&2
  exit 1
fi

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
PROMOTION_DIR="${OUT_DIR:-${PROJECT_DIR}/artifacts/ops/attestation_promotion_required/run_${RUN_ID}}"
RETENTION_REPORT="${PROMOTION_DIR}/retention_report.json"
BUNDLE_REPORT="${PROMOTION_DIR}/evidence_bundle/bundle_report.json"
REPLAY_REPORT="${PROMOTION_DIR}/offline_replay_report.json"
REHEARSAL_REPORT="${PROMOTION_DIR}/rehearsal_report.json"
mkdir -p "${PROMOTION_DIR}"

echo "[rehearsal] start"
echo "[rehearsal] run_id=${RUN_ID}"
echo "[rehearsal] promotion_dir=${PROMOTION_DIR}"

(
  cd "${CONTRACTS_DIR}"
  BASE_MODE=required OUT_DIR="${PROMOTION_DIR}" ./scripts/promote_attestation_release.sh
)

python3 "${CONTRACTS_DIR}/scripts/bundle_attestation_evidence.py" \
  --run-dir "${PROMOTION_DIR}" \
  --signing-key "${EVIDENCE_SIGNING_KEY_PEM}" \
  --public-key "${EVIDENCE_SIGNING_PUB_PEM}" \
  --require-signing \
  --output "${BUNDLE_REPORT}"

python3 "${CONTRACTS_DIR}/scripts/replay_attestation_promotion_offline.py" \
  --promotion-report "${PROMOTION_DIR}/promotion_report.json" \
  --require-signed-bundle \
  --output "${REPLAY_REPORT}"

RETENTION_POLICY="${RETENTION_POLICY:-${CONTRACTS_DIR}/scripts/attestation_evidence_retention_policy_v1.json}"
python3 "${CONTRACTS_DIR}/scripts/enforce_attestation_bundle_retention.py" \
  --policy "${RETENTION_POLICY}" \
  --apply \
  --json-report "${RETENTION_REPORT}"

python3 - \
  "${REHEARSAL_REPORT}" \
  "${RUN_ID}" \
  "${PROMOTION_DIR}" \
  "${BUNDLE_REPORT}" \
  "${REPLAY_REPORT}" \
  "${RETENTION_REPORT}" <<'PY'
import json
import sys
from datetime import datetime, timezone

(
    out_file,
    run_id,
    promotion_dir,
    bundle_report_file,
    replay_report_file,
    retention_report_file,
) = sys.argv[1:]

bundle = json.loads(open(bundle_report_file, "r", encoding="utf-8").read())
replay = json.loads(open(replay_report_file, "r", encoding="utf-8").read())
retention = json.loads(open(retention_report_file, "r", encoding="utf-8").read())
ok = bool(bundle.get("ok") and replay.get("ok") and retention.get("ok"))

report = {
    "schema": "nucleusdb/attestation-required-rehearsal-report/v1",
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "run_id": run_id,
    "promotion_dir": promotion_dir,
    "ok": ok,
    "bundle_report": bundle_report_file,
    "replay_report": replay_report_file,
    "retention_report": retention_report_file,
}
with open(out_file, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")
print(json.dumps(report, indent=2, sort_keys=True))
if not ok:
    raise SystemExit(1)
PY

echo "[rehearsal] PASS"
echo "[rehearsal] report=${REHEARSAL_REPORT}"
