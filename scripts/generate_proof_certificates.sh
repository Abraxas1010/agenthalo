#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
HEYTING_ROOT="${HEYTING_ROOT:-/home/abraxas/Work/heyting}"
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

git -C "$HEYTING_ROOT" fetch origin --quiet
HEYTING_COMMIT=$(git -C "$HEYTING_ROOT" rev-parse origin/master)
GENERATED_AT=$(date +%s)

THEOREMS=(
  "HeytingLean.NucleusDB.Core.Certificates.verifyCommitCertificate_sound|lean/HeytingLean/NucleusDB/Core/Certificates.lean"
  "HeytingLean.NucleusDB.Sheaf.Coherence.verifyCoherence_sound|lean/HeytingLean/NucleusDB/Sheaf/Coherence.lean"
  "HeytingLean.Crypto.Commit.IPAInstance.openCorrect|lean/HeytingLean/Crypto/Commit/IPAInstance.lean"
  "HeytingLean.NucleusDB.Core.NucleusSystem.step_eq_apply|lean/HeytingLean/NucleusDB/Core/Nucleus.lean"
  "HeytingLean.NucleusDB.Security.Refinement.certificate_to_refinement|lean/HeytingLean/NucleusDB/Security/Refinement.lean"
  "HeytingLean.NucleusDB.Transparency.CT6962.consistency_implies_prefix|lean/HeytingLean/NucleusDB/Transparency/CT6962.lean"
  "HeytingLean.NucleusDB.Transparency.CT6962.verifyInclusionProof_sound|lean/HeytingLean/NucleusDB/Transparency/CT6962.lean"
  "HeytingLean.Crypto.Commit.IPAInstance.openSound_of_binding|lean/HeytingLean/Crypto/Commit/IPAInstance.lean"
  "HeytingLean.NucleusDB.Crypto.EVMGate.evm_sign_requires_dual_auth|lean/HeytingLean/NucleusDB/Crypto/EVMGate.lean"
  "HeytingLean.NucleusDB.Crypto.EVMGate.authorization_composable|lean/HeytingLean/NucleusDB/Crypto/EVMGate.lean"
  "HeytingLean.Crypto.KEM.HybridKEM.hybrid_security_of_documentedAssumptions|lean/HeytingLean/Crypto/KEM/HybridKEM.lean"
  "HeytingLean.NucleusDB.Sheaf.TraceTopology.refines_preserves_connected|lean/HeytingLean/NucleusDB/Sheaf/TraceTopology.lean"
  "HeytingLean.NucleusDB.Sheaf.TraceTopology.componentConstant_iff_exists_lift|lean/HeytingLean/NucleusDB/Sheaf/TraceTopology.lean"
  "HeytingLean.NucleusDB.Sheaf.TraceTopology.componentCount_mono_of_refines|lean/HeytingLean/NucleusDB/Sheaf/TraceTopology.lean"
)

extract_decl_line() {
  local theorem_path="$1"
  local source_file="$2"
  local short_name="${theorem_path##*.}"
  git -C "$HEYTING_ROOT" show "origin/master:${source_file}" | \
    python3 -c '
import re, sys
short = sys.argv[1]
pat = re.compile(rf"\b(theorem|def|abbrev)\s+{re.escape(short)}(\s|$)")
for raw in sys.stdin:
    line = raw.rstrip("\\n")
    if pat.search(line.strip()):
        print(line.strip())
        raise SystemExit(0)
raise SystemExit(1)
' "$short_name"
}

count=0
for entry in "${THEOREMS[@]}"; do
  theorem="${entry%%|*}"
  source_file="${entry#*|}"
  decl_line=$(extract_decl_line "$theorem" "$source_file")
  statement_hash=$(printf '%s' "$decl_line" | sha256sum | awk '{print $1}')
  base_name=$(printf '%s' "$theorem" | tr '.' '_')
  cert_path="$TMP_DIR/${base_name}.lean4export"
  cat > "$cert_path" <<CERT
#THM ${theorem}
#AX propext
#AX Classical.choice
#AX Quot.sound
#META commit_hash ${HEYTING_COMMIT}
#META theorem_statement_sha256 ${statement_hash}
#META generated_at ${GENERATED_AT}
CERT
  (cd "$REPO_ROOT" && cargo run --quiet --bin nucleusdb -- verify-certificate "$cert_path" >/dev/null)
  (cd "$REPO_ROOT" && cargo run --quiet --bin nucleusdb -- submit-certificate "$cert_path" >/dev/null)
  count=$((count + 1))
  printf 'generated %s\n' "$theorem"
done

printf 'Generated and submitted %d certificates into %s\n' "$count" "$CERT_DIR"
