import NucleusDB.TrustLayer.CompositeCab.GlobalCab

/-!
# NucleusDB.TrustLayer.CompositeCab.Circuit

Witness/public-input surface for composite CAB proof systems.
-/

namespace NucleusDB.TrustLayer.CompositeCab

/-- Composite CAB witness values emitted by the prover lane. -/
structure CompositeCabWitness where
  cab : CompositeCab
  bridgeConservationOk : Bool
  replaySafe : Bool
  circuitDigest : String

/-- Public inputs consumed by an on-chain verifier. -/
structure CompositeCabPublicInputs where
  expectedDigest : String
  coveredChainCount : Nat
  deriving Repr

/-- Circuit satisfaction predicate for the composite CAB witness/public pair. -/
def compositeCabCircuitSatisfied
    (w : CompositeCabWitness) (pub : CompositeCabPublicInputs) : Prop :=
  w.circuitDigest = pub.expectedDigest ∧
    w.cab.coveredChains.card = pub.coveredChainCount ∧
    w.bridgeConservationOk = true ∧
    w.replaySafe = true

end NucleusDB.TrustLayer.CompositeCab
