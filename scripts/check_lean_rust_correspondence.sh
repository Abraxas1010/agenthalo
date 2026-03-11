#!/usr/bin/env bash
# Verify the standalone NucleusDB Lean↔Rust correspondences that still have a
# concrete runtime mirror in this repo. Parent-repo-only correspondences from the
# deleted AgentHALO surfaces are intentionally out of scope here.

set -euo pipefail

EXIT=0

check_marker() {
  local theorem="$1"
  local rust_file="$2"
  local marker="$3"
  if ! grep -q "$marker" "$rust_file" 2>/dev/null; then
    echo "MISSING: Lean theorem '$theorem' expects marker '$marker' in $rust_file"
    EXIT=1
  fi
}

check_function() {
  local fn_name="$1"
  local rust_file="$2"
  if ! grep -Eq "fn[[:space:]]+${fn_name}[[:space:]]*\\(" "$rust_file" 2>/dev/null; then
    echo "MISSING: expected function '$fn_name' in $rust_file"
    EXIT=1
  fi
}

check_lean_theorem() {
  local theorem="$1"
  local lean_file="$2"
  if ! grep -Eq "theorem[[:space:]]+${theorem}([[:space:]]|$)" "$lean_file" 2>/dev/null; then
    echo "MISSING: expected theorem '$theorem' in $lean_file"
    EXIT=1
  fi
}

# Identity correspondence
check_marker "genesis_derivation_deterministic" "src/genesis.rs" "T5"
check_function "derive_p2p_identity" "src/genesis.rs"
check_lean_theorem "genesis_derivation_deterministic" \
  "lean/NucleusDB/Comms/Identity/GenesisDerivation.lean"

check_function "did_from_genesis_seed" "src/did.rs"
check_lean_theorem "did_document_wellformed" \
  "lean/NucleusDB/Comms/Identity/DIDDocumentSpec.lean"

echo ""
if [ "$EXIT" -eq 0 ]; then
  echo "All standalone Lean↔Rust correspondence markers found."
else
  echo "Some standalone correspondence markers are missing."
fi
exit "$EXIT"
