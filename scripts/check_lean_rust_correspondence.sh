#!/usr/bin/env bash
# Verify key Lean theorem surfaces have corresponding Rust implementation markers.
# This is a correspondence smoke test, not a formal refinement proof.

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

# Identity correspondence
check_marker "genesis_derivation_deterministic" "src/halo/genesis_seed.rs" "T5"
check_marker "did_document_wellformed" "src/halo/did.rs" "T7"

# ZK correspondence
check_marker "credential_completeness" "src/halo/zk_credential.rs" "T17"
check_marker "credential_soundness" "src/halo/zk_credential.rs" "T18"
check_marker "anon_credential_anonymity" "src/halo/zk_credential.rs" "T19"

# Access-control correspondence
check_marker "capability_requires_valid_signature" "src/pod/acl.rs" "CapabilityToken"
check_marker "authorized_mutation_requires_full_chain" "src/pod/acl.rs" "AuthChain"

echo ""
if [ "$EXIT" -eq 0 ]; then
  echo "All Lean↔Rust correspondence markers found."
else
  echo "Some correspondence markers are missing."
fi
exit "$EXIT"
