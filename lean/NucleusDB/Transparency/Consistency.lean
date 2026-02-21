import NucleusDB.Transparency.LogModel

namespace HeytingLean
namespace NucleusDB
namespace Transparency

/-- Minimal consistency-proof payload linking an old and new STH size. -/
structure ConsistencyProof where
  oldSize : Nat
  newSize : Nat

/-- Model-level consistency verifier. -/
def verifyConsistency (old new : STH) (π : ConsistencyProof) : Prop :=
  π.oldSize = old.size ∧ π.newSize = new.size ∧ old.size ≤ new.size

/-- Verified consistency implies append-only extension. -/
theorem verifyConsistency_implies_extends
    {old new : STH} {π : ConsistencyProof}
    (h : verifyConsistency old new π) :
    Extends old new :=
  h.2.2

end Transparency
end NucleusDB
end HeytingLean
