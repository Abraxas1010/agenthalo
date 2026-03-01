import Mathlib.CategoryTheory.Category.Basic

namespace HeytingLean
namespace NucleusDB
namespace Core

open CategoryTheory

universe u v

/-- Minimal nucleus-style transition interface for NucleusDB state evolution. -/
structure NucleusSystem where
  State : Type u
  Delta : Type v
  apply : State → Delta → State

namespace NucleusSystem

/-- One transition step in the nucleus system. -/
def step (S : NucleusSystem) (s : S.State) (d : S.Delta) : S.State :=
  S.apply s d

@[simp] theorem step_eq_apply (S : NucleusSystem) (s : S.State) (d : S.Delta) :
    S.step s d = S.apply s d :=
  rfl

/-- `NucleusSystem` plus compositional delta operations for categorical lifts. -/
structure NucleusComposableSystem extends NucleusSystem where
  noop : Delta
  seq : Delta → Delta → Delta
  apply_noop : ∀ s : State, apply s noop = s
  apply_seq : ∀ s : State, ∀ d₁ d₂ : Delta,
    apply s (seq d₁ d₂) = apply (apply s d₁) d₂

/-- Reachability by a single compositional delta witness. -/
def NucleusReachable (S : NucleusComposableSystem) (a b : S.State) : Prop :=
  ∃ d : S.Delta, S.apply a d = b

/-- Type-level hom wrapper for thin reachability categories. -/
abbrev NucleusReachableHom (S : NucleusComposableSystem) (a b : S.State) : Type :=
  PLift (NucleusReachable S a b)

instance (S : NucleusComposableSystem) : Category S.State where
  Hom a b := NucleusReachableHom S a b
  id a := ⟨⟨S.noop, S.apply_noop a⟩⟩
  comp f g := by
    refine ⟨?_⟩
    rcases f.down with ⟨d₁, hd₁⟩
    rcases g.down with ⟨d₂, hd₂⟩
    refine ⟨S.seq d₁ d₂, ?_⟩
    calc
      S.apply _ (S.seq d₁ d₂) = S.apply (S.apply _ d₁) d₂ := by
        simpa using S.apply_seq _ d₁ d₂
      _ = S.apply _ d₂ := by simp [hd₁]
      _ = _ := hd₂
  id_comp := by
    intro _ _ f
    apply Subsingleton.elim
  comp_id := by
    intro _ _ f
    apply Subsingleton.elim
  assoc := by
    intro _ _ _ _ f g h
    apply Subsingleton.elim

/-- Canonical composable lift: batch deltas as lists with append composition. -/
def batchComposable (S : NucleusSystem) : NucleusComposableSystem where
  State := S.State
  Delta := List S.Delta
  apply := fun s ds => ds.foldl S.apply s
  noop := []
  seq := List.append
  apply_noop := by
    intro s
    rfl
  apply_seq := by
    intro s d₁ d₂
    simp [List.foldl_append]

end NucleusSystem

end Core
end NucleusDB
end HeytingLean
