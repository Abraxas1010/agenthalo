namespace HeytingLean
namespace Crypto
namespace Commit
namespace Spec

universe u v w

namespace Vec

/-- Minimal vector commitment interface stub used by the standalone NucleusDB
formal surface. -/
structure Scheme where
  Idx : Type u
  Val : Type v
  Com : Type w
  Proof : Type w
  commit : (Idx → Val) → Com
  openAt : (Idx → Val) → Idx → Proof
  verifyAt : Com → Idx → Val → Proof → Prop

/-- Correctness axiom shape for point openings at committed vectors. -/
def Scheme.OpenCorrect (S : Scheme) : Prop :=
  ∀ (vec : S.Idx → S.Val) (i : S.Idx),
    S.verifyAt (S.commit vec) i (vec i) (S.openAt vec i)

/-- Soundness axiom shape for point openings at committed vectors. -/
def Scheme.OpenSound (S : Scheme) : Prop :=
  ∀ (vec : S.Idx → S.Val) (i : S.Idx) (y : S.Val) (π : S.Proof),
    S.verifyAt (S.commit vec) i y π → vec i = y

end Vec
end Spec
end Commit
end Crypto
end HeytingLean
