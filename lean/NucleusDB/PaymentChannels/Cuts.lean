import Mathlib.Algebra.Order.BigOperators.Group.Finset
import NucleusDB.PaymentChannels.Wealth

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators
open Sym2

/-!
# Blockchain.PaymentChannels.Cuts

Cut-based constraints for feasible wealth distributions.

For any subset `S` of vertices, the sum of wealth in `S` is constrained by:
- internal channel capacity inside `S`; and
- the cut capacity across the boundary of `S`.

This mirrors the core “cut interval” obstruction used in payment feasibility arguments.
-/

universe u

namespace Cuts

variable {V : Type u} [DecidableEq V]

def IsInternal (S : Finset V) : Sym2 V → Prop :=
  Sym2.lift ⟨(fun u v => u ∈ S ∧ v ∈ S), by
    intro a b
    apply propext
    simp [and_comm]⟩

def IsCut (S : Finset V) : Sym2 V → Prop :=
  Sym2.lift ⟨(fun u v => (u ∈ S ∧ v ∉ S) ∨ (u ∉ S ∧ v ∈ S)), by
    intro a b
    apply propext
    constructor <;> intro h
    · rcases h with h | h
      · exact Or.inr ⟨h.2, h.1⟩
      · exact Or.inl ⟨h.2, h.1⟩
    · rcases h with h | h
      · exact Or.inr ⟨h.2, h.1⟩
      · exact Or.inl ⟨h.2, h.1⟩⟩

omit [DecidableEq V] in
@[simp] lemma isInternal_mk (S : Finset V) (u v : V) :
    IsInternal S (s(u, v)) ↔ (u ∈ S ∧ v ∈ S) := Iff.rfl

omit [DecidableEq V] in
@[simp] lemma isCut_mk (S : Finset V) (u v : V) :
    IsCut S (s(u, v)) ↔ ((u ∈ S ∧ v ∉ S) ∨ (u ∉ S ∧ v ∈ S)) := Iff.rfl

instance (S : Finset V) : DecidablePred (IsInternal (V := V) S) := by
  intro e
  refine Sym2.recOnSubsingleton (motive := fun e => Decidable (IsInternal (V := V) S e)) e ?_
  rintro ⟨u, v⟩
  have : Decidable (u ∈ S ∧ v ∈ S) := by
    infer_instance
  simpa using this

instance (S : Finset V) : DecidablePred (IsCut (V := V) S) := by
  intro e
  refine Sym2.recOnSubsingleton (motive := fun e => Decidable (IsCut (V := V) S e)) e ?_
  rintro ⟨u, v⟩
  have : Decidable ((u ∈ S ∧ v ∉ S) ∨ (u ∉ S ∧ v ∈ S)) := by
    infer_instance
  simpa using this

def internalEdges (G : ChannelGraph V) (S : Finset V) : Finset (Sym2 V) :=
  G.edges.filter (IsInternal S)

def cutEdges (G : ChannelGraph V) (S : Finset V) : Finset (Sym2 V) :=
  G.edges.filter (IsCut S)

def internalCapacity (G : ChannelGraph V) (S : Finset V) : Cap :=
  ∑ e ∈ internalEdges (G := G) S, G.cap e

def cutCapacity (G : ChannelGraph V) (S : Finset V) : Cap :=
  ∑ e ∈ cutEdges (G := G) S, G.cap e

def edgeContribution (S : Finset V) (l : LiquidityFn V) (e : Sym2 V) : Cap :=
  ∑ x ∈ S, l e x

def wealthIn (G : ChannelGraph V) (l : LiquidityFn V) (S : Finset V) : Cap :=
  ∑ x ∈ S, (Wealth.pi G l) x

def edgeBound (G : ChannelGraph V) (S : Finset V) (e : Sym2 V) : Cap :=
  if IsInternal S e then
    G.cap e
  else if IsCut S e then
    G.cap e
  else
    0

omit [DecidableEq V] in
lemma isInternal_not_isCut (S : Finset V) (e : Sym2 V) :
    IsInternal S e → ¬ IsCut S e := by
  classical
  cases e using Sym2.ind with
  | _ u v =>
      intro hI hC
      have hI' : u ∈ S ∧ v ∈ S := by simpa using hI
      have hC' : (u ∈ S ∧ v ∉ S) ∨ (u ∉ S ∧ v ∈ S) := by simpa using hC
      rcases hC' with hC' | hC'
      · exact hC'.2 hI'.2
      · exact hC'.1 hI'.1

lemma sum_edgeBound_eq (G : ChannelGraph V) (S : Finset V) :
    (∑ e ∈ G.edges, edgeBound G S e) = internalCapacity G S + cutCapacity G S := by
  classical
  have h_disj : ∀ e, IsInternal S e → ¬ IsCut S e :=
    fun e => isInternal_not_isCut (S := S) e
  have hPoint :
      ∀ e : Sym2 V,
        edgeBound G S e =
          (if IsInternal S e then G.cap e else 0) + (if IsCut S e then G.cap e else 0) := by
    intro e
    by_cases hI : IsInternal S e
    · have hC : ¬ IsCut S e := h_disj e hI
      simp [edgeBound, hI, hC]
    · by_cases hC : IsCut S e
      · simp [edgeBound, hI, hC]
      · simp [edgeBound, hI, hC]
  calc
    (∑ e ∈ G.edges, edgeBound G S e)
        = ∑ e ∈ G.edges,
            ((if IsInternal S e then G.cap e else 0) + (if IsCut S e then G.cap e else 0)) := by
            refine Finset.sum_congr rfl ?_
            intro e _he
            simp [hPoint e]
    _   = (∑ e ∈ G.edges, if IsInternal S e then G.cap e else 0)
          + (∑ e ∈ G.edges, if IsCut S e then G.cap e else 0) := by
            simp [Finset.sum_add_distrib]
    _   = internalCapacity G S + cutCapacity G S := by
            -- Rewrite both terms via `Finset.sum_filter`.
            have hInt :
                (∑ e ∈ G.edges, if IsInternal S e then G.cap e else 0)
                  = ∑ e ∈ G.edges with IsInternal S e, G.cap e := by
              simpa using
                (Finset.sum_filter (s := G.edges) (p := IsInternal S) (f := G.cap)).symm
            have hCut :
                (∑ e ∈ G.edges, if IsCut S e then G.cap e else 0)
                  = ∑ e ∈ G.edges with IsCut S e, G.cap e := by
              simpa using
                (Finset.sum_filter (s := G.edges) (p := IsCut S) (f := G.cap)).symm
            -- Now unfold `internalCapacity`/`cutCapacity`.
            simp [internalCapacity, internalEdges, cutCapacity, cutEdges, hInt, hCut]

lemma wealthIn_eq_sum_edgeContribution (G : ChannelGraph V) (l : LiquidityFn V) (S : Finset V) :
    wealthIn G l S = ∑ e ∈ G.edges, edgeContribution S l e := by
  classical
  -- `wealthIn` is a double sum; swap the order.
  have hswap :
      (∑ x ∈ S, ∑ e ∈ G.edges, l e x) = ∑ e ∈ G.edges, ∑ x ∈ S, l e x := by
    exact (Finset.sum_comm (s := S) (t := G.edges) (f := fun x e => l e x))
  simp [wealthIn, Wealth.pi, Wealth.wealth, edgeContribution, hswap]

lemma edgeContribution_eq_cap_of_internal {G : ChannelGraph V} {l : LiquidityFn V}
    (hLG : l ∈ LiquidityFn.LG G) {S : Finset V} {e : Sym2 V}
    (he : e ∈ G.edges) (hI : IsInternal S e) :
    edgeContribution S l e = G.cap e := by
  classical
  rcases hLG with ⟨hCon, hNI, _hOG⟩
  cases e using Sym2.ind with
  | _ u v =>
      have huv : u ≠ v := G.ne_of_mem_edges (u := u) (v := v) he
      have hI' : u ∈ S ∧ v ∈ S := by simpa using hI
      have huS : u ∈ S := hI'.1
      have hvS : v ∈ S := hI'.2
      -- Expand the sum by erasing `u` then `v`.
      have hsum_u :
          (∑ x ∈ S, l (s(u, v)) x) =
            (∑ x ∈ S.erase u, l (s(u, v)) x) + l (s(u, v)) u := by
        simpa using
          (Finset.sum_erase_add (s := S) (f := fun x => l (s(u, v)) x) (a := u) huS).symm
      have hvErase : v ∈ S.erase u := by
        exact (Finset.mem_erase).2 ⟨huv.symm, hvS⟩
      have hsum_v :
          (∑ x ∈ S.erase u, l (s(u, v)) x) =
            (∑ x ∈ (S.erase u).erase v, l (s(u, v)) x) + l (s(u, v)) v := by
        simpa using
          (Finset.sum_erase_add (s := S.erase u) (f := fun x => l (s(u, v)) x) (a := v) hvErase).symm
      have hrest :
          (∑ x ∈ (S.erase u).erase v, l (s(u, v)) x) = 0 := by
        refine Finset.sum_eq_zero ?_
        intro x hx
        have hx' : x ∈ S.erase u := (Finset.mem_erase.mp hx).2
        have hxne_v : x ≠ v := (Finset.mem_erase.mp hx).1
        have hxne_u : x ≠ u := (Finset.mem_erase.mp hx').1
        have hxnot : x ∉ s(u, v) := by
          simp [Sym2.mem_iff, hxne_u, hxne_v]
        exact hNI (s(u, v)) x hxnot
      have hConUV : l (s(u, v)) u + l (s(u, v)) v = G.cap (s(u, v)) := hCon u v he
      -- Assemble.
      calc
        edgeContribution S l (s(u, v))
            = ∑ x ∈ S, l (s(u, v)) x := rfl
        _   = (∑ x ∈ S.erase u, l (s(u, v)) x) + l (s(u, v)) u := hsum_u
        _   = ((∑ x ∈ (S.erase u).erase v, l (s(u, v)) x) + l (s(u, v)) v) + l (s(u, v)) u := by
              simp [hsum_v, add_assoc]
        _   = (∑ x ∈ (S.erase u).erase v, l (s(u, v)) x) + (l (s(u, v)) u + l (s(u, v)) v) := by
              simp [add_left_comm, add_comm]
        _   = l (s(u, v)) u + l (s(u, v)) v := by
              simp [hrest]
        _   = G.cap (s(u, v)) := hConUV

lemma edgeContribution_le_cap_of_cut {G : ChannelGraph V} {l : LiquidityFn V}
    (hLG : l ∈ LiquidityFn.LG G) {S : Finset V} {e : Sym2 V}
    (he : e ∈ G.edges) (hC : IsCut S e) :
    edgeContribution S l e ≤ G.cap e := by
  classical
  rcases hLG with ⟨hCon, hNI, _hOG⟩
  cases e using Sym2.ind with
  | _ u v =>
      have huv : u ≠ v := G.ne_of_mem_edges (u := u) (v := v) he
      have hC' : (u ∈ S ∧ v ∉ S) ∨ (u ∉ S ∧ v ∈ S) := by simpa using hC
      have hConUV : l (s(u, v)) u + l (s(u, v)) v = G.cap (s(u, v)) := hCon u v he
      rcases hC' with ⟨huS, hvS⟩ | ⟨huS, hvS⟩
      · -- `u ∈ S`, `v ∉ S` ⇒ the contribution is `λ e u`
        have hsum_u :
            (∑ x ∈ S, l (s(u, v)) x) =
              (∑ x ∈ S.erase u, l (s(u, v)) x) + l (s(u, v)) u := by
          simpa using
            (Finset.sum_erase_add (s := S) (f := fun x => l (s(u, v)) x) (a := u) huS).symm
        have hrest : (∑ x ∈ S.erase u, l (s(u, v)) x) = 0 := by
          refine Finset.sum_eq_zero ?_
          intro x hx
          have hxne_u : x ≠ u := (Finset.mem_erase.mp hx).1
          have hxS : x ∈ S := (Finset.mem_erase.mp hx).2
          have hxne_v : x ≠ v := by
            intro hEq
            subst hEq
            exact hvS hxS
          have hxnot : x ∉ s(u, v) := by
            simp [Sym2.mem_iff, hxne_u, hxne_v]
          exact hNI (s(u, v)) x hxnot
        have hEq : edgeContribution S l (s(u, v)) = l (s(u, v)) u := by
          simp [edgeContribution, hsum_u, hrest]
        -- Bound by `cap` via conservation.
        have hle : l (s(u, v)) u ≤ l (s(u, v)) u + l (s(u, v)) v := Nat.le_add_right _ _
        simpa [hEq, hConUV] using hle
      · -- `v ∈ S`, `u ∉ S` ⇒ symmetric case
        have hsum_v :
            (∑ x ∈ S, l (s(u, v)) x) =
              (∑ x ∈ S.erase v, l (s(u, v)) x) + l (s(u, v)) v := by
          simpa using
            (Finset.sum_erase_add (s := S) (f := fun x => l (s(u, v)) x) (a := v) hvS).symm
        have hrest : (∑ x ∈ S.erase v, l (s(u, v)) x) = 0 := by
          refine Finset.sum_eq_zero ?_
          intro x hx
          have hxne_v : x ≠ v := (Finset.mem_erase.mp hx).1
          have hxS : x ∈ S := (Finset.mem_erase.mp hx).2
          have hxne_u : x ≠ u := by
            intro hEq
            subst hEq
            exact huS hxS
          have hxnot : x ∉ s(u, v) := by
            simp [Sym2.mem_iff, hxne_u, hxne_v]
          exact hNI (s(u, v)) x hxnot
        have hEq : edgeContribution S l (s(u, v)) = l (s(u, v)) v := by
          simp [edgeContribution, hsum_v, hrest]
        have hle : l (s(u, v)) v ≤ l (s(u, v)) u + l (s(u, v)) v := Nat.le_add_left _ _
        -- `Nat.le_add_left` gives `λ v ≤ λ u + λ v`.
        simpa [hEq, add_comm, hConUV] using hle

lemma edgeContribution_eq_zero_of_not_internal_not_cut {l : LiquidityFn V} {S : Finset V} {e : Sym2 V}
    (hNI : LiquidityFn.NonIncidentZero l) (hI : ¬ IsInternal S e) (hC : ¬ IsCut S e) :
    edgeContribution S l e = 0 := by
  classical
  cases e using Sym2.ind with
  | _ u v =>
      have hOutside : u ∉ S ∧ v ∉ S := by
        by_cases hu : u ∈ S
        · have hv : v ∈ S := by
            by_contra hv
            apply hC
            exact Or.inl ⟨hu, hv⟩
          exact False.elim (hI ⟨hu, hv⟩)
        · have hv : v ∉ S := by
            by_contra hv
            apply hC
            exact Or.inr ⟨hu, hv⟩
          exact ⟨hu, hv⟩
      refine Finset.sum_eq_zero ?_
      intro x hx
      have hxne_u : x ≠ u := by
        intro hEq
        subst hEq
        exact hOutside.1 hx
      have hxne_v : x ≠ v := by
        intro hEq
        subst hEq
        exact hOutside.2 hx
      have hxnot : x ∉ s(u, v) := by
        simp [Sym2.mem_iff, hxne_u, hxne_v]
      exact hNI (s(u, v)) x hxnot

lemma edgeContribution_le_edgeBound {G : ChannelGraph V} {l : LiquidityFn V}
    (hLG : l ∈ LiquidityFn.LG G) (S : Finset V) {e : Sym2 V} (he : e ∈ G.edges) :
    edgeContribution S l e ≤ edgeBound G S e := by
  classical
  have hNI : LiquidityFn.NonIncidentZero l := hLG.2.1
  by_cases hI : IsInternal S e
  · have hEq : edgeContribution S l e = G.cap e :=
      edgeContribution_eq_cap_of_internal (G := G) (l := l) hLG he hI
    simp [edgeBound, hEq, hI]
  · by_cases hC : IsCut S e
    · have hle : edgeContribution S l e ≤ G.cap e :=
        edgeContribution_le_cap_of_cut (G := G) (l := l) hLG he hC
      simpa [edgeBound, hI, hC] using hle
    · have hEq0 : edgeContribution S l e = 0 :=
        edgeContribution_eq_zero_of_not_internal_not_cut (l := l) (S := S) (e := e) hNI hI hC
      simp [edgeBound, hEq0, hI, hC]

theorem cutInterval {G : ChannelGraph V} {l : LiquidityFn V} (hLG : l ∈ LiquidityFn.LG G)
    (S : Finset V) :
    internalCapacity G S ≤ wealthIn G l S ∧ wealthIn G l S ≤ internalCapacity G S + cutCapacity G S := by
  classical
  -- Rewrite `wealthIn` as a sum over edges of edge contributions.
  have hWealth :
      wealthIn G l S = ∑ e ∈ G.edges, edgeContribution S l e :=
    wealthIn_eq_sum_edgeContribution (G := G) (l := l) (S := S)

  -- Lower bound: internal edges contribute their full capacity, and the rest is nonnegative.
  have hInternal :
      internalCapacity G S = ∑ e ∈ internalEdges (G := G) S, edgeContribution S l e := by
    refine Finset.sum_congr rfl ?_
    intro e he
    have he' : e ∈ G.edges.filter (IsInternal S) := by simpa [internalEdges] using he
    have heEdges : e ∈ G.edges := (Finset.mem_filter.mp he').1
    have hI : IsInternal S e := (Finset.mem_filter.mp he').2
    simpa using (edgeContribution_eq_cap_of_internal (G := G) (l := l) hLG heEdges hI).symm
  have hSub : internalEdges (G := G) S ⊆ G.edges :=
    by simp [internalEdges]
  have hLeLower :
      internalCapacity G S ≤ ∑ e ∈ G.edges, edgeContribution S l e := by
    -- Use monotonicity of `Finset.sum` over subsets in `ℕ`.
    simpa [hInternal] using
      (Finset.sum_le_sum_of_subset (f := fun e => edgeContribution S l e) hSub)

  -- Upper bound: each edge contribution is bounded by `edgeBound`.
  have hLeUpper' :
      (∑ e ∈ G.edges, edgeContribution S l e) ≤ ∑ e ∈ G.edges, edgeBound G S e := by
    refine Finset.sum_le_sum ?_
    intro e he
    exact edgeContribution_le_edgeBound (G := G) (l := l) hLG S he
  have hBoundSum :
      (∑ e ∈ G.edges, edgeBound G S e) = internalCapacity G S + cutCapacity G S :=
    sum_edgeBound_eq (G := G) S

  constructor
  · simpa [hWealth] using hLeLower
  · simpa [hWealth, hBoundSum] using hLeUpper'

end Cuts

end PaymentChannels
end NucleusDB
