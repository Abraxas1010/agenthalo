import HeytingLean.Crypto.Commit.Spec

namespace HeytingLean
namespace NucleusDB
namespace Commitment

open HeytingLean.Crypto.Commit.Spec

/-- Runtime commitment backends used by NucleusDB. -/
inductive BackendTag
  | ipa
  | kzg
  | binaryMerkle
deriving DecidableEq, Repr

/-- Adapter over the existing vector commitment spec interface. -/
structure VCAdapter where
  scheme : Vec.Scheme

namespace VCAdapter

/-- Canonical opening-check predicate at a given index for a given vector. -/
def openingHolds (A : VCAdapter) (v : A.scheme.Idx → A.scheme.Val) (i : A.scheme.Idx) : Prop :=
  A.scheme.verifyAt (A.scheme.commit v) i (v i) (A.scheme.openAt v i)

/-- Opening checks hold whenever the underlying scheme satisfies `OpenCorrect`. -/
theorem openingHolds_of_openCorrect
    (A : VCAdapter)
    (h : Vec.Scheme.OpenCorrect A.scheme)
    (v : A.scheme.Idx → A.scheme.Val)
    (i : A.scheme.Idx) :
    A.openingHolds v i :=
  h v i

end VCAdapter

end Commitment
end NucleusDB
end HeytingLean
