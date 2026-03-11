namespace HeytingLean
namespace NucleusDB
namespace TrustLayer

/-- Public attestation slots mirrored from the Groth16 payload. -/
structure AttestationInputs where
  merkleLo : Rat
  merkleHi : Rat
  digestLo : Rat
  digestHi : Rat
  eventCount : Rat
  deriving Repr

/-- Witness-side slot assignment for the five equality constraints. -/
structure AttestationWitness where
  merkleLo : Rat
  merkleHi : Rat
  digestLo : Rat
  digestHi : Rat
  eventCount : Rat
  deriving Repr

/-- Canonical witness copies the public attestation values into witness slots. -/
def canonicalWitness (input : AttestationInputs) : AttestationWitness :=
  { merkleLo := input.merkleLo
    merkleHi := input.merkleHi
    digestLo := input.digestLo
    digestHi := input.digestHi
    eventCount := input.eventCount }

def publicSlots (input : AttestationInputs) : List Rat :=
  [input.merkleLo, input.merkleHi, input.digestLo, input.digestHi, input.eventCount]

def witnessSlots (witness : AttestationWitness) : List Rat :=
  [witness.merkleLo, witness.merkleHi, witness.digestLo, witness.digestHi, witness.eventCount]

/-- Five equality gates: every public slot must equal its witness counterpart. -/
def circuitSatisfied (input : AttestationInputs) (witness : AttestationWitness) : Prop :=
  witness.merkleLo = input.merkleLo ∧
    witness.merkleHi = input.merkleHi ∧
    witness.digestLo = input.digestLo ∧
    witness.digestHi = input.digestHi ∧
    witness.eventCount = input.eventCount

/-- T31: the attestation circuit is satisfiable by the canonical witness. -/
theorem attestation_circuit_satisfiable (input : AttestationInputs) :
    circuitSatisfied input (canonicalWitness input) := by
  simp [circuitSatisfied, canonicalWitness]

/-- T32: the Merkle root and digest halves are bound identically. -/
theorem attestation_circuit_output_correct
    (input : AttestationInputs) (witness : AttestationWitness)
    (h : circuitSatisfied input witness) :
    publicSlots input = witnessSlots witness := by
  rcases h with ⟨h0, h1, h2, h3, h4⟩
  simp [publicSlots, witnessSlots, h0, h1, h2, h3, h4]

end TrustLayer
end NucleusDB
end HeytingLean
