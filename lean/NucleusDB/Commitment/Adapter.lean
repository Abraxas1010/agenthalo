namespace HeytingLean
namespace Crypto
namespace Commit
namespace Spec

namespace Vec

/-- Minimal vector-commitment scheme interface used by standalone NucleusDB specs. -/
structure Scheme where
  Idx : Type
  Val : Type
  Com : Type
  Proof : Type
  commit : (Idx → Val) → Com
  openAt : (Idx → Val) → Idx → Proof
  verifyAt : Com → Idx → Val → Proof → Prop

namespace Scheme

/-- Completeness of openings against committed vectors. -/
def OpenCorrect (S : Vec.Scheme) : Prop :=
  ∀ (v : S.Idx → S.Val) (i : S.Idx),
    S.verifyAt (S.commit v) i (v i) (S.openAt v i)

/-- Soundness of accepted openings at a committed point. -/
def OpenSound (S : Vec.Scheme) : Prop :=
  ∀ (v : S.Idx → S.Val) (i : S.Idx) (y : S.Val) (π : S.Proof),
    S.verifyAt (S.commit v) i y π → v i = y

end Scheme
end Vec

end Spec
end Commit
end Crypto

namespace NucleusDB
namespace Commitment

open HeytingLean.Crypto.Commit.Spec

/-- Runtime commitment backends used by NucleusDB. -/
inductive BackendTag
  | ipa
  | kzg
  | binaryMerkle
deriving DecidableEq, Repr

/-- Adapter over the standalone vector commitment spec interface. -/
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
