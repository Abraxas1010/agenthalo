import NucleusDB.Adversarial.ForkEvidence

/-!
# NucleusDB.Adversarial.ByzantineTolerance

Byzantine witness-capacity lemmas transferred from the NucleusPOD adversarial
family and adapted to NucleusDB's checkpoint fork model.
-/

namespace HeytingLean
namespace NucleusDB
namespace Adversarial

/-- Effective honest-witness capacity under `faultyWitnesses` corruption. -/
def witnessCapacity (totalWitnesses faultyWitnesses : Nat) : Nat :=
  totalWitnesses - faultyWitnesses

/-- Honest witness capacity is always bounded by total witness count. -/
theorem witness_capacity_le_total (totalWitnesses faultyWitnesses : Nat) :
    witnessCapacity totalWitnesses faultyWitnesses ≤ totalWitnesses := by
  exact Nat.sub_le totalWitnesses faultyWitnesses

/-- Capacity decomposition: honest capacity plus faulty count recovers total. -/
theorem witness_capacity_recovery (n f : Nat) (hf : f ≤ n) :
    witnessCapacity n f + f = n := by
  exact Nat.sub_add_cancel hf

/-- If a fork is observed, zero-faulty witness assumptions are inconsistent. -/
theorem fork_without_faulty_witness_impossible
    (a b : SignedCheckpoint)
    (totalWitnesses faultyWitnesses : Nat)
    (hIntegrity : faultyWitnesses = 0 → ¬ Fork a b)
    (hFork : Fork a b) :
    faultyWitnesses > 0 := by
  have _hTotalWitnesses : Nat := totalWitnesses
  by_cases hZero : faultyWitnesses = 0
  · exact False.elim ((hIntegrity hZero) hFork)
  · exact Nat.pos_of_ne_zero hZero

/-- Fork evidence implies either faulty witnesses exist or fork integrity was violated. -/
theorem fork_requires_byzantine_witness
    (a b : SignedCheckpoint)
    (hFork : Fork a b)
    (totalWitnesses faultyWitnesses : Nat)
    (hQuorum : witnessCapacity totalWitnesses faultyWitnesses > 0)
    (hIntegrity : faultyWitnesses = 0 → ¬ Fork a b) :
    faultyWitnesses > 0 ∨ ¬ Fork a b := by
  have _hQuorum : witnessCapacity totalWitnesses faultyWitnesses > 0 := hQuorum
  have hFaulty : faultyWitnesses > 0 :=
    fork_without_faulty_witness_impossible a b totalWitnesses faultyWitnesses hIntegrity hFork
  exact Or.inl hFaulty

end Adversarial
end NucleusDB
end HeytingLean
