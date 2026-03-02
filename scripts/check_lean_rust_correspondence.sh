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
check_marker "genesis_derivation_deterministic" "src/halo/genesis_seed.rs" "T5"
check_function "derive_p2p_identity" "src/halo/genesis_seed.rs"
check_lean_theorem "genesis_derivation_deterministic" \
  "lean/NucleusDB/Comms/Identity/GenesisDerivation.lean"

check_marker "did_document_wellformed" "src/halo/did.rs" "T7"
check_function "did_from_genesis_seed" "src/halo/did.rs"
check_lean_theorem "did_document_wellformed" \
  "lean/NucleusDB/Comms/Identity/DIDDocumentSpec.lean"

# ZK correspondence
check_marker "credential_completeness" "src/halo/zk_credential.rs" "T17"
check_function "prove_credential" "src/halo/zk_credential.rs"
check_lean_theorem "credential_completeness" \
  "lean/NucleusDB/Comms/ZK/CredentialSpec.lean"

check_marker "credential_soundness" "src/halo/zk_credential.rs" "T18"
check_function "verify_credential_proof" "src/halo/zk_credential.rs"
check_lean_theorem "credential_soundness" \
  "lean/NucleusDB/Comms/ZK/CredentialSpec.lean"

check_marker "anon_credential_anonymity" "src/halo/zk_credential.rs" "T19"
check_function "verify_anonymous_membership_proof" "src/halo/zk_credential.rs"
check_lean_theorem "anon_credential_anonymity" \
  "lean/NucleusDB/Comms/ZK/AnonymousCredentialSpec.lean"

# Access-control correspondence
check_marker "capability_requires_valid_signature" "src/pod/acl.rs" "CapabilityToken"
check_function "compute_id" "src/pod/acl.rs"
check_lean_theorem "capability_requires_valid_signature" \
  "lean/NucleusDB/Comms/AccessControl/CapabilityToken.lean"

check_marker "authorized_mutation_requires_full_chain" "src/pod/acl.rs" "AuthChain"
check_function "can_control" "src/pod/acl.rs"
check_lean_theorem "authorized_mutation_requires_full_chain" \
  "lean/NucleusDB/Comms/AccessControl/AuthChain.lean"

# DIDComm/protocol correspondence
check_marker "rust_authcrypt_refines_protocol" "src/halo/didcomm.rs" "T21"
check_function "authcrypt_gate_accepts" "src/halo/didcomm.rs"
check_lean_theorem "rust_authcrypt_refines_protocol" \
  "lean/NucleusDB/Security/DIDCommRefinement.lean"

check_marker "rust_anoncrypt_refines_protocol" "src/halo/didcomm.rs" "T22"
check_function "anoncrypt_gate_accepts" "src/halo/didcomm.rs"
check_lean_theorem "rust_anoncrypt_refines_protocol" \
  "lean/NucleusDB/Security/DIDCommRefinement.lean"

check_marker "topic_isolation_spec" "src/halo/p2p_discovery.rs" "T23"
check_function "is_allowed_capability_topic" "src/halo/p2p_discovery.rs"
check_lean_theorem "known_topics_are_allowed" \
  "lean/NucleusDB/Comms/Protocol/TopicIsolationSpec.lean"

check_marker "credential_binding_accepts" "src/halo/didcomm_handler.rs" "T24"
check_function "validate_credential_attachment_for_request" "src/halo/didcomm_handler.rs"
check_lean_theorem "credential_request_requires_verified_proof" \
  "lean/NucleusDB/Comms/Protocol/ZKBindingSpec.lean"

check_marker "builtin_accepts_implies_receipt_valid" "src/halo/zk_compute.rs" "T25"
check_function "execute_builtin_guest" "src/halo/zk_compute.rs"
check_lean_theorem "builtin_accepts_implies_receipt_valid" \
  "lean/NucleusDB/Comms/ZK/VerifiableComputation.lean"

echo ""
if [ "$EXIT" -eq 0 ]; then
  echo "All Lean↔Rust correspondence markers found."
else
  echo "Some correspondence markers are missing."
fi
exit "$EXIT"
