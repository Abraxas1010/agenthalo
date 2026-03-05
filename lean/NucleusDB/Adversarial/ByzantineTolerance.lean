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
    (faultyWitnesses : Nat)
    (hIntegrity : faultyWitnesses = 0 → ¬ Fork a b)
    (hFork : Fork a b) :
    faultyWitnesses > 0 := by
  by_cases hZero : faultyWitnesses = 0
  · exact False.elim ((hIntegrity hZero) hFork)
  · exact Nat.pos_of_ne_zero hZero

/-- Positive witness capacity implies faults are strictly below total witnesses. -/
theorem positive_capacity_implies_faults_below_total
    (totalWitnesses faultyWitnesses : Nat)
    (hQuorum : witnessCapacity totalWitnesses faultyWitnesses > 0) :
    faultyWitnesses < totalWitnesses := by
  have hNotGe : ¬ totalWitnesses ≤ faultyWitnesses := by
    intro hle
    have hSubZero : witnessCapacity totalWitnesses faultyWitnesses = 0 := by
      simp [witnessCapacity, Nat.sub_eq_zero_of_le hle]
    have hNotPos : ¬ witnessCapacity totalWitnesses faultyWitnesses > 0 := by
      simp [hSubZero]
    exact hNotPos hQuorum
  exact Nat.lt_of_not_ge hNotGe

/-- Fork evidence forces existence of at least one faulty witness under the
runtime integrity assumption. -/
theorem fork_requires_byzantine_witness
    (a b : SignedCheckpoint)
    (hFork : Fork a b)
    (totalWitnesses faultyWitnesses : Nat)
    (hQuorum : witnessCapacity totalWitnesses faultyWitnesses > 0)
    (hIntegrity : faultyWitnesses = 0 → ¬ Fork a b) :
    faultyWitnesses > 0 := by
  have _hBound : faultyWitnesses < totalWitnesses :=
    positive_capacity_implies_faults_below_total totalWitnesses faultyWitnesses hQuorum
  exact fork_without_faulty_witness_impossible a b faultyWitnesses hIntegrity hFork

/-- Compatibility corollary preserving the original disjunctive shape. -/
theorem fork_requires_byzantine_witness_or_no_fork
    (a b : SignedCheckpoint)
    (hFork : Fork a b)
    (totalWitnesses faultyWitnesses : Nat)
    (hQuorum : witnessCapacity totalWitnesses faultyWitnesses > 0)
    (hIntegrity : faultyWitnesses = 0 → ¬ Fork a b) :
    faultyWitnesses > 0 ∨ ¬ Fork a b := by
  exact Or.inl (fork_requires_byzantine_witness a b hFork totalWitnesses faultyWitnesses hQuorum hIntegrity)

end Adversarial
end NucleusDB
end HeytingLean
