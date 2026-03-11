import Mathlib.Data.Fintype.Basic
import Mathlib.Data.Fintype.Card
import Mathlib.Logic.Relation

namespace HeytingLean
namespace NucleusDB
namespace Sheaf
namespace TraceTopology

open Relation

universe u

/-- Finite tool metric space for trace-topology analysis. -/
structure ToolMetricSpace (α : Type u) extends Fintype α where
  decEq : DecidableEq α
  dist : α → α → Nat
  dist_self : ∀ x, dist x x = 0
  dist_symm : ∀ x y, dist x y = dist y x

attribute [instance] ToolMetricSpace.decEq

instance {α : Type u} (M : ToolMetricSpace α) : Fintype α := M.toFintype

def Neighbor {α : Type u} (M : ToolMetricSpace α) (t : Nat) : α → α → Prop :=
  fun x y => M.dist x y ≤ t

abbrev Connected {α : Type u} (M : ToolMetricSpace α) (t : Nat) : α → α → Prop :=
  ReflTransGen (Neighbor M t)

def Refines {α : Type u} (M N : ToolMetricSpace α) : Prop :=
  ∀ x y, N.dist x y ≤ M.dist x y

theorem neighbor_symm {α : Type u} (M : ToolMetricSpace α) (t : Nat) {x y : α} :
    Neighbor M t x y → Neighbor M t y x := by
  intro h
  simpa [Neighbor, M.dist_symm] using h

theorem connected_symm {α : Type u} (M : ToolMetricSpace α) (t : Nat) {x y : α} :
    Connected M t x y → Connected M t y x := by
  have hsymm : Symmetric (Neighbor M t) := fun _ _ => neighbor_symm M t
  intro h
  exact Relation.ReflTransGen.symmetric hsymm h

theorem connected_is_equivalence {α : Type u} (M : ToolMetricSpace α) (t : Nat) :
    Equivalence (Connected M t) :=
  ⟨Relation.reflexive_reflTransGen, connected_symm M t, ReflTransGen.trans⟩

def connectedSetoid {α : Type u} (M : ToolMetricSpace α) (t : Nat) : Setoid α :=
  Setoid.mk _ (connected_is_equivalence M t)

abbrev ConnectedComponent {α : Type u} (M : ToolMetricSpace α) (t : Nat) : Type u :=
  Quotient (connectedSetoid M t)

noncomputable instance instDecidableRelConnected {α : Type u}
    (M : ToolMetricSpace α) (t : Nat) : DecidableRel (Connected M t) :=
  Classical.decRel _

noncomputable instance instFintypeConnectedComponent {α : Type u}
    (M : ToolMetricSpace α) (t : Nat) : Fintype (ConnectedComponent M t) := by
  classical
  exact @Quotient.fintype α M.toFintype (connectedSetoid M t) (instDecidableRelConnected M t)

noncomputable def componentCount {α : Type u} (M : ToolMetricSpace α) (t : Nat) : Nat :=
  Fintype.card (ConnectedComponent M t)

def ComponentConstant {α : Type u} (M : ToolMetricSpace α) (t : Nat) {β : Type*}
    (f : α → β) : Prop :=
  ∀ ⦃x y : α⦄, Connected M t x y → f x = f y

def liftConnectedComponent {α : Type u} (M : ToolMetricSpace α) (t : Nat) {β : Type*}
    (f : α → β) (hf : ComponentConstant M t f) :
    ConnectedComponent M t → β :=
  Quotient.lift f (by
    intro x y hxy
    exact hf hxy)

@[simp] theorem liftConnectedComponent_mk {α : Type u} (M : ToolMetricSpace α) (t : Nat)
    {β : Type*} (f : α → β) (hf : ComponentConstant M t f) (x : α) :
    liftConnectedComponent M t f hf (Quotient.mk'' x) = f x := by
  rfl

theorem componentConstant_iff_exists_lift {α : Type u} (M : ToolMetricSpace α) (t : Nat)
    {β : Type*} (f : α → β) :
    ComponentConstant M t f ↔
      ∃ g : ConnectedComponent M t → β, ∀ x : α, g (Quotient.mk'' x) = f x := by
  constructor
  · intro hf
    refine ⟨liftConnectedComponent M t f hf, ?_⟩
    intro x
    simp [liftConnectedComponent_mk]
  · rintro ⟨g, hg⟩ x y hxy
    have hx : g (Quotient.mk'' x) = f x := hg x
    have hy : g (Quotient.mk'' y) = f y := hg y
    have hq : (Quotient.mk'' x : ConnectedComponent M t) = Quotient.mk'' y :=
      Quotient.sound hxy
    calc
      f x = g (Quotient.mk'' x) := hx.symm
      _ = g (Quotient.mk'' y) := by simp [hq]
      _ = f y := hy

theorem liftConnectedComponent_unique {α : Type u} (M : ToolMetricSpace α) (t : Nat)
    {β : Type*} (f : α → β) (hf : ComponentConstant M t f)
    (g : ConnectedComponent M t → β)
    (hg : ∀ x : α, g (Quotient.mk'' x) = f x) :
    g = liftConnectedComponent M t f hf := by
  funext q
  refine Quotient.inductionOn q ?_
  intro x
  simpa [liftConnectedComponent_mk] using hg x

theorem connected_mono_of_scale {α : Type u} (M : ToolMetricSpace α) {s t : Nat}
    (hst : s ≤ t) {x y : α} :
    Connected M s x y → Connected M t x y := by
  intro hxy
  induction hxy using Relation.ReflTransGen.trans_induction_on with
  | refl _ =>
      exact ReflTransGen.refl
  | @single x y hstep =>
      exact ReflTransGen.single (le_trans hstep hst)
  | trans _ _ ihab ihbc =>
      exact ReflTransGen.trans ihab ihbc

theorem refines_preserves_connected {α : Type u} {M N : ToolMetricSpace α}
    (href : Refines M N) {t : Nat} {x y : α} :
    Connected M t x y → Connected N t x y := by
  intro hxy
  induction hxy using Relation.ReflTransGen.trans_induction_on with
  | refl _ =>
      exact ReflTransGen.refl
  | @single x y hstep =>
      exact ReflTransGen.single (le_trans (href x y) hstep)
  | trans _ _ ihab ihbc =>
      exact ReflTransGen.trans ihab ihbc

def connectedComponentMapOfScale {α : Type u} (M : ToolMetricSpace α) {s t : Nat}
    (hst : s ≤ t) :
    ConnectedComponent M s → ConnectedComponent M t :=
  Quotient.map' id (fun _ _ hxy => connected_mono_of_scale M hst hxy)

theorem connectedComponentMapOfScale_surjective {α : Type u} (M : ToolMetricSpace α)
    {s t : Nat} (hst : s ≤ t) :
    Function.Surjective (connectedComponentMapOfScale M hst) := by
  intro q
  refine Quotient.inductionOn q ?_
  intro x
  exact ⟨Quotient.mk'' x, rfl⟩

def connectedComponentMapOfRefines {α : Type u} {M N : ToolMetricSpace α}
    (href : Refines M N) {t : Nat} :
    ConnectedComponent M t → ConnectedComponent N t :=
  Quotient.map' id (fun _ _ hxy => refines_preserves_connected href hxy)

theorem connectedComponentMapOfRefines_surjective {α : Type u}
    {M N : ToolMetricSpace α} (href : Refines M N) {t : Nat} :
    Function.Surjective (connectedComponentMapOfRefines href (t := t)) := by
  intro q
  refine Quotient.inductionOn q ?_
  intro x
  exact ⟨Quotient.mk'' x, rfl⟩

theorem componentCount_mono_of_scale {α : Type u} (M : ToolMetricSpace α)
    {s t : Nat} (hst : s ≤ t) :
    componentCount M t ≤ componentCount M s := by
  classical
  simpa [componentCount] using
    Fintype.card_le_of_surjective _ (connectedComponentMapOfScale_surjective M hst)

theorem componentCount_mono_of_refines {α : Type u}
    {M N : ToolMetricSpace α} (href : Refines M N) {t : Nat} :
    componentCount N t ≤ componentCount M t := by
  classical
  simpa [componentCount] using
    Fintype.card_le_of_surjective _ (connectedComponentMapOfRefines_surjective href)

end TraceTopology
end Sheaf
end NucleusDB
end HeytingLean
