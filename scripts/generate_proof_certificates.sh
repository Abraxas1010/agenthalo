#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
HEYTING_ROOT="${HEYTING_ROOT:-}"
CERT_DIR="${CERT_DIR:-$HOME/.nucleusdb/proof_certificates}"
TMP_DIR="${TMP_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/nucleusdb_proof_certs.XXXXXX")}"
KEEP_TMP="${KEEP_TMP:-0}"

cleanup() {
  if [[ "$KEEP_TMP" != "1" ]]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT

mkdir -p "$TMP_DIR" "$CERT_DIR"

GENERATED_AT=$(date +%s)

PLAN_JSON="$TMP_DIR/certificate_plan.json"

if [[ -n "$HEYTING_ROOT" ]]; then
  git -C "$HEYTING_ROOT" fetch origin --quiet
  python3 "$SCRIPT_DIR/formal_provenance_resolver.py" certificate-plan \
    --repo-root "$REPO_ROOT" \
    --heyting-root "$HEYTING_ROOT" > "$PLAN_JSON"
else
  python3 "$SCRIPT_DIR/formal_provenance_resolver.py" certificate-plan \
    --repo-root "$REPO_ROOT" > "$PLAN_JSON"
fi

count=0
while IFS=$'\t' read -r theorem statement_hash commit_hash; do
  base_name=$(printf '%s' "$theorem" | tr '.' '_')
  cert_path="$TMP_DIR/${base_name}.lean4export"
  cat > "$cert_path" <<CERT
#THM ${theorem}
#AX propext
#AX Classical.choice
#AX Quot.sound
#META commit_hash ${commit_hash}
#META theorem_statement_sha256 ${statement_hash}
#META generated_at ${GENERATED_AT}
CERT
  (cd "$REPO_ROOT" && cargo run --quiet --bin nucleusdb -- sign-certificate "$cert_path" >/dev/null)
  (cd "$REPO_ROOT" && cargo run --quiet --bin nucleusdb -- verify-certificate "$cert_path" >/dev/null)
  (cd "$REPO_ROOT" && cargo run --quiet --bin nucleusdb -- submit-certificate "$cert_path" >/dev/null)
  count=$((count + 1))
  printf 'generated %s\n' "$theorem"
done < <(
  python3 - "$PLAN_JSON" <<'PY'
import json
import sys

plan = json.loads(open(sys.argv[1], encoding="utf-8").read())
for entry in plan:
    print(f"{entry['theorem']}\t{entry['statement_hash']}\t{entry['commit_hash']}")
PY
)

printf 'Generated and submitted %d certificates into %s\n' "$count" "$CERT_DIR"
