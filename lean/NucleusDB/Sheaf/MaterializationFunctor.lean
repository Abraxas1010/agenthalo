import NucleusDB.Sheaf.Coherence
import Mathlib.CategoryTheory.Category.Basic
import Mathlib.CategoryTheory.Functor.Basic
import Mathlib.CategoryTheory.Discrete.Basic
import Mathlib.CategoryTheory.NatTrans

namespace HeytingLean
namespace NucleusDB
namespace Sheaf

open CategoryTheory

universe u v w

/-- Abstract materialization map plus transport-compatibility law. -/
structure MaterializationFunctor (State : Type u) (Idx : Type v) (Val : Type w) where
  toVector : State → Idx → Val
  transports : State → State → Prop
  naturality : ∀ s t, transports s t → toVector s = toVector t

/-- Naturality can be used directly as extensional equality after transport. -/
theorem materialize_transport_eq
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M : MaterializationFunctor State Idx Val)
    {s t : State}
    (h : M.transports s t) :
    M.toVector s = M.toVector t :=
  M.naturality s t h

/-- Transport relation laws needed to view transports as a thin category. -/
class TransportLaws {State : Type u} (R : State → State → Prop) : Prop where
  refl : ∀ s : State, R s s
  trans : ∀ {a b c : State}, R a b → R b c → R a c

def transportCategory
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M : MaterializationFunctor State Idx Val)
    [TransportLaws M.transports] :
    Category State where
  Hom s t := PLift (M.transports s t)
  id s := ⟨TransportLaws.refl s⟩
  comp f g := ⟨TransportLaws.trans f.down g.down⟩
  id_comp := by
    intro _ _ f
    apply Subsingleton.elim
  comp_id := by
    intro _ _ f
    apply Subsingleton.elim
  assoc := by
    intro _ _ _ _ f g h
    apply Subsingleton.elim

/-- Categorical uplift: materialization as a functor into a discrete target category. -/
def materializationDiscreteFunctor
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M : MaterializationFunctor State Idx Val) :
    Discrete State ⥤ Discrete (Idx → Val) :=
  Discrete.functor (fun s => (⟨M.toVector s⟩ : Discrete (Idx → Val)))

/-- Identity natural transformation for a materialization functor. -/
def materializationIdentityNat
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M : MaterializationFunctor State Idx Val) :
    materializationDiscreteFunctor M ⟶ materializationDiscreteFunctor M :=
  𝟙 _

end Sheaf
end NucleusDB
end HeytingLean
