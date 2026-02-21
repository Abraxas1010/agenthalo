import NucleusDB.Adversarial.Witness

namespace HeytingLean
namespace NucleusDB
namespace Adversarial

/-- Signed checkpoint view observed by a client. -/
structure SignedCheckpoint where
  height : Nat
  prevRoot : String
  stateRoot : String
  sthRoot : String

/-- Fork evidence: same position, conflicting state roots. -/
def Fork (a b : SignedCheckpoint) : Prop :=
  a.height = b.height ∧ a.prevRoot = b.prevRoot ∧ a.stateRoot ≠ b.stateRoot

theorem fork_symmetric {a b : SignedCheckpoint} :
    Fork a b → Fork b a := by
  intro h
  refine ⟨h.1.symm, h.2.1.symm, ?_⟩
  exact Ne.symm h.2.2

end Adversarial
end NucleusDB
end HeytingLean
