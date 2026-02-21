import NucleusDB.Commitment.Adapter
import HeytingLean.Crypto.Commit.Spec

namespace HeytingLean
namespace NucleusDB
namespace Commitment

open HeytingLean.Crypto.Commit.Spec

/-- A committed vector state paired with its commitment. -/
structure CommittedVector (A : VCAdapter) where
  vector : A.scheme.Idx → A.scheme.Val
  commitment : A.scheme.Com
  commitmentEq : commitment = A.scheme.commit vector

/-- Authenticated point-opening proposition for a committed vector. -/
def AuthenticatedAt (A : VCAdapter) (cv : CommittedVector A)
    (i : A.scheme.Idx) (y : A.scheme.Val) (π : A.scheme.Proof) : Prop :=
  A.scheme.verifyAt cv.commitment i y π

/-- Any valid opening against `commit vector` is equal to the true value, under `OpenSound`. -/
theorem authenticated_value_unique
    (A : VCAdapter)
    (hSound : Vec.Scheme.OpenSound A.scheme)
    (cv : CommittedVector A)
    (i : A.scheme.Idx)
    (y : A.scheme.Val)
    (π : A.scheme.Proof)
    (hAuth : AuthenticatedAt A cv i y π)
    (hCommit : cv.commitment = A.scheme.commit cv.vector) :
    cv.vector i = y := by
  have h' : A.scheme.verifyAt (A.scheme.commit cv.vector) i y π := by
    simpa [AuthenticatedAt, hCommit] using hAuth
  exact hSound cv.vector i y π h'

end Commitment
end NucleusDB
end HeytingLean
