import Mathlib.CategoryTheory.Category.Basic
import Mathlib.CategoryTheory.Functor.Basic
import NucleusDB.Genesis.Entropy.Combiner
import NucleusDB.Genesis.Entropy.Gate

open CategoryTheory

namespace HeytingLean
namespace NucleusDB
namespace Genesis
namespace Entropy

/-- Abstract harvest state in the Genesis ceremony. -/
structure HarvestState where
  successes : Nat
  remoteSuccesses : Nat
  deriving DecidableEq, Repr

/-- Morphisms are monotone extensions of harvest evidence. -/
structure HarvestHom (a b : HarvestState) : Type where
  succLe : a.successes ≤ b.successes
  remoteLe : a.remoteSuccesses ≤ b.remoteSuccesses

instance : Category HarvestState where
  Hom a b := HarvestHom a b
  id a := { succLe := Nat.le_refl _, remoteLe := Nat.le_refl _ }
  comp f g := {
    succLe := Nat.le_trans f.succLe g.succLe
    remoteLe := Nat.le_trans f.remoteLe g.remoteLe
  }
  id_comp := by
    intro X Y f
    cases f
    rfl
  comp_id := by
    intro X Y f
    cases f
    rfl
  assoc := by
    intro W X Y Z f g h
    cases f
    cases g
    cases h
    rfl

def authorized_delta_morphism
    {a b : HarvestState}
    (hSucc : a.successes ≤ b.successes)
    (hRemote : a.remoteSuccesses ≤ b.remoteSuccesses) :
    (a ⟶ b) := by
  exact { succLe := hSucc, remoteLe := hRemote }

theorem gate_monotone_under_morphisms
    {a b : HarvestState} (h : a ⟶ b)
    (ha : gateUnlock a.successes a.remoteSuccesses) :
    gateUnlock b.successes b.remoteSuccesses := by
  have haMin : a.successes ≥ sourceMinSuccess := by
    simpa [gateUnlock, policyPass, defaultPolicy, sourceMinSuccess] using ha.1
  have haRemote : a.remoteSuccesses > 0 := by
    have hOr := ha.2
    cases hOr with
    | inl hFalse =>
        exact False.elim (hFalse (by simp [defaultPolicy]))
    | inr hPos =>
        exact hPos
  refine And.intro ?_ ?_
  · simpa [sourceMinSuccess] using Nat.le_trans haMin h.succLe
  · right
    exact Nat.lt_of_lt_of_le haRemote h.remoteLe

end Entropy
end Genesis
end NucleusDB
end HeytingLean
