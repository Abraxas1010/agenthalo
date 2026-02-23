import Mathlib.Data.Finset.Fold
import Mathlib.Data.Fintype.Basic
import NucleusDB.PaymentChannels.Payments

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators
open Sym2

/-!
# Blockchain.PaymentChannels.Algorithmic

Phase 4: a computable (finite) checker for membership in the feasible wealth set `WG`.

We avoid implementing max-flow/min-cut in this phase. Instead, for finite `V` we build a finset
`WGFinset` by exhaustive enumeration:

- each undirected edge contributes a finite set of possible per-vertex wealth contributions
  (splitting its capacity between its endpoints), and
- `WGFinset` is the Minkowski sum of these per-edge sets.

This yields a correct decision procedure for `w ∈ Wealth.WG G` and for payment feasibility.
-/

universe u

namespace Algorithmic

variable {V : Type u} [DecidableEq V] [Fintype V]

abbrev WealthDist (V : Type u) : Type u := V → Cap

def zeroWealth : WealthDist V := fun _ => 0

def edgeWealth (G : ChannelGraph V) (u v : V) (a : Cap) : WealthDist V :=
  fun x =>
    if x = u then
      a
    else if x = v then
      G.cap (s(u, v)) - a
    else
      0

def edgeWealthChoices (G : ChannelGraph V) (u v : V) : Finset (WealthDist V) :=
  (Finset.range (G.cap (s(u, v)) + 1)).image (edgeWealth (V := V) G u v)

theorem edgeWealthChoices_comm (G : ChannelGraph V) (u v : V) :
    edgeWealthChoices (V := V) G u v = edgeWealthChoices (V := V) G v u := by
  classical
  by_cases huv : u = v
  · subst huv
    rfl
  · ext w
    constructor
    · intro hw
      rcases Finset.mem_image.mp hw with ⟨a, ha, rfl⟩
      have ha' : a < G.cap (s(u, v)) + 1 := by
        simpa [Finset.mem_range] using ha
      have hle : a ≤ G.cap (s(u, v)) := Nat.le_of_lt_succ ha'
      refine Finset.mem_image.mpr ?_
      refine ⟨G.cap (s(u, v)) - a, ?_, ?_⟩
      · have hlt : G.cap (s(u, v)) - a < G.cap (s(v, u)) + 1 := by
          simpa [Sym2.eq_swap] using (Nat.lt_succ_iff.mpr (Nat.sub_le (G.cap (s(u, v))) a))
        simpa [Finset.mem_range] using hlt
      · funext x
        by_cases hxu : x = u
        · have hxv : x ≠ v := by
            intro hxv
            exact huv (hxu.symm.trans hxv)
          simp [edgeWealth, hxu, Sym2.eq_swap, Nat.sub_sub_self hle, huv]
        · by_cases hxv : x = v
          · have hvu : v ≠ u := by
              intro h
              exact huv h.symm
            simp [edgeWealth, hxv, hvu]
          · simp [edgeWealth, hxu, hxv]
    · intro hw
      rcases Finset.mem_image.mp hw with ⟨a, ha, rfl⟩
      have ha' : a < G.cap (s(v, u)) + 1 := by
        simpa [Finset.mem_range] using ha
      have hle : a ≤ G.cap (s(v, u)) := Nat.le_of_lt_succ ha'
      refine Finset.mem_image.mpr ?_
      refine ⟨G.cap (s(v, u)) - a, ?_, ?_⟩
      · have hlt : G.cap (s(v, u)) - a < G.cap (s(u, v)) + 1 := by
          simpa [Sym2.eq_swap] using (Nat.lt_succ_iff.mpr (Nat.sub_le (G.cap (s(v, u))) a))
        simpa [Finset.mem_range] using hlt
      · funext x
        by_cases hxv : x = v
        · have hxu : x ≠ u := by
            intro hxu
            exact huv (hxu.symm.trans hxv)
          have hvu : v ≠ u := by
            intro h
            exact huv h.symm
          have hleUV : a ≤ G.cap (s(u, v)) := by
            simpa [Sym2.eq_swap] using hle
          simp [edgeWealth, hxv, Sym2.eq_swap, Nat.sub_sub_self hleUV, hvu]
        · by_cases hxu : x = u
          · have hvu : v ≠ u := by
              intro h
              exact huv h.symm
            simp [edgeWealth, hxu, Sym2.eq_swap, huv]
          · simp [edgeWealth, hxu, hxv]

def edgeWealths (G : ChannelGraph V) : Sym2 V → Finset (WealthDist V) :=
  Sym2.lift ⟨edgeWealthChoices (V := V) G, edgeWealthChoices_comm (G := G)⟩

def addWealth (A B : Finset (WealthDist V)) : Finset (WealthDist V) :=
  (A.product B).image (fun p => p.1 + p.2)

omit [DecidableEq V] in
lemma mem_addWealth {A B : Finset (WealthDist V)} {w : WealthDist V} :
    w ∈ addWealth (V := V) A B ↔ ∃ w1 ∈ A, ∃ w2 ∈ B, w1 + w2 = w := by
  classical
  constructor
  · intro hw
    rcases Finset.mem_image.mp hw with ⟨p, hp, rfl⟩
    refine ⟨p.1, (Finset.mem_product.mp hp).1, p.2, (Finset.mem_product.mp hp).2, rfl⟩
  · rintro ⟨w1, hw1, w2, hw2, rfl⟩
    refine Finset.mem_image.mpr ?_
    exact ⟨(w1, w2), Finset.mem_product.mpr ⟨hw1, hw2⟩, rfl⟩

omit [DecidableEq V] in
theorem addWealth_comm : Std.Commutative (addWealth (V := V)) := by
  classical
  constructor
  intro A B
  ext w
  constructor
  · intro hw
    rcases Finset.mem_image.mp hw with ⟨p, hp, rfl⟩
    have hp' : (p.2, p.1) ∈ B.product A := by
      exact Finset.mem_product.mpr ⟨(Finset.mem_product.mp hp).2, (Finset.mem_product.mp hp).1⟩
    refine Finset.mem_image.mpr ?_
    refine ⟨(p.2, p.1), hp', ?_⟩
    ext x
    simp [add_comm]
  · intro hw
    rcases Finset.mem_image.mp hw with ⟨p, hp, rfl⟩
    have hp' : (p.2, p.1) ∈ A.product B := by
      exact Finset.mem_product.mpr ⟨(Finset.mem_product.mp hp).2, (Finset.mem_product.mp hp).1⟩
    refine Finset.mem_image.mpr ?_
    refine ⟨(p.2, p.1), hp', ?_⟩
    ext x
    simp [add_comm]

omit [DecidableEq V] in
theorem addWealth_assoc : Std.Associative (addWealth (V := V)) := by
  classical
  constructor
  intro A B C
  ext w
  constructor
  · intro hw
    rcases (mem_addWealth (V := V) (A := addWealth (V := V) A B) (B := C) (w := w)).1 hw with
      ⟨wAB, hwAB, wC, hwC, rfl⟩
    rcases (mem_addWealth (V := V) (A := A) (B := B) (w := wAB)).1 hwAB with ⟨wA, hwA, wB, hwB, rfl⟩
    refine (mem_addWealth (V := V) (A := A) (B := addWealth (V := V) B C) (w := (wA + wB) + wC)).2 ?_
    refine ⟨wA, hwA, wB + wC, ?_, by simp [add_assoc]⟩
    exact (mem_addWealth (V := V) (A := B) (B := C) (w := wB + wC)).2 ⟨wB, hwB, wC, hwC, rfl⟩
  · intro hw
    rcases (mem_addWealth (V := V) (A := A) (B := addWealth (V := V) B C) (w := w)).1 hw with
      ⟨wA, hwA, wBC, hwBC, rfl⟩
    rcases (mem_addWealth (V := V) (A := B) (B := C) (w := wBC)).1 hwBC with ⟨wB, hwB, wC, hwC, rfl⟩
    refine (mem_addWealth (V := V) (A := addWealth (V := V) A B) (B := C) (w := wA + (wB + wC))).2 ?_
    refine ⟨wA + wB, ?_, wC, hwC, by simp [add_assoc]⟩
    exact (mem_addWealth (V := V) (A := A) (B := B) (w := wA + wB)).2 ⟨wA, hwA, wB, hwB, rfl⟩

section

local instance : Std.Commutative (addWealth (V := V)) := addWealth_comm (V := V)
local instance : Std.Associative (addWealth (V := V)) := addWealth_assoc (V := V)

def WGFinsetOn (G : ChannelGraph V) (E : Finset (Sym2 V)) : Finset (WealthDist V) :=
  Finset.fold (op := addWealth (V := V)) ({zeroWealth (V := V)} : Finset (WealthDist V))
    (edgeWealths (V := V) G) E

def WGFinset (G : ChannelGraph V) : Finset (WealthDist V) :=
  WGFinsetOn (V := V) G G.edges

def wgBool (G : ChannelGraph V) (w : WealthDist V) : Bool :=
  decide (w ∈ WGFinset (V := V) G)

def paymentFeasibleBool (G : ChannelGraph V) (w : WealthDist V) (i j : V) (a : Cap) : Bool :=
  decide (a ≤ w i) && wgBool (V := V) G (Payments.pay w i j a)

namespace LiquidityFn

def ConservationOn (G : ChannelGraph V) (E : Finset (Sym2 V)) (l : LiquidityFn V) : Prop :=
  ∀ u v : V, s(u, v) ∈ E → l (s(u, v)) u + l (s(u, v)) v = G.cap (s(u, v))

def OffEdgesZeroOn (E : Finset (Sym2 V)) (l : LiquidityFn V) : Prop :=
  ∀ e : Sym2 V, e ∉ E → ∀ x : V, l e x = 0

def FeasibleOn (G : ChannelGraph V) (E : Finset (Sym2 V)) (l : LiquidityFn V) : Prop :=
  ConservationOn (V := V) G E l ∧
    HeytingLean.Blockchain.PaymentChannels.LiquidityFn.NonIncidentZero (V := V) l ∧
    OffEdgesZeroOn (V := V) E l

def LGOn (G : ChannelGraph V) (E : Finset (Sym2 V)) : Set (LiquidityFn V) :=
  {l | FeasibleOn (V := V) G E l}

end LiquidityFn

def piOn (E : Finset (Sym2 V)) (l : LiquidityFn V) : WealthDist V :=
  fun v => ∑ e ∈ E, l e v

def edgeLiquidity (e : Sym2 V) (w : WealthDist V) : LiquidityFn V :=
  fun e' x => if e' = e then w x else 0

def zeroEdge (e : Sym2 V) (l : LiquidityFn V) : LiquidityFn V :=
  fun e' x => if e' = e then 0 else l e' x

omit [DecidableEq V] [Fintype V] in
lemma piOn_empty (l : LiquidityFn V) : piOn (V := V) (∅ : Finset (Sym2 V)) l = zeroWealth (V := V) := by
  classical
  funext v
  simp [piOn, zeroWealth]

omit [DecidableEq V] [Fintype V] in
lemma piOn_singleton (e : Sym2 V) (l : LiquidityFn V) :
    piOn (V := V) ({e} : Finset (Sym2 V)) l = fun v => l e v := by
  classical
  funext v
  simp [piOn]

omit [DecidableEq V] [Fintype V] in
lemma piOn_add (E : Finset (Sym2 V)) (l₁ l₂ : LiquidityFn V) :
    piOn (V := V) E (l₁ + l₂) = piOn (V := V) E l₁ + piOn (V := V) E l₂ := by
  classical
  funext v
  simp [piOn, Finset.sum_add_distrib]

omit [Fintype V] in
lemma piOn_insert (E : Finset (Sym2 V)) (e : Sym2 V) (h : e ∉ E) (l : LiquidityFn V) :
    piOn (V := V) (insert e E) l = (fun v => l e v) + piOn (V := V) E l := by
  classical
  funext v
  simp [piOn, Finset.sum_insert, h]

lemma edgeWealth_support {G : ChannelGraph V} {e : Sym2 V} {w : WealthDist V}
    (hw : w ∈ edgeWealths (V := V) G e) :
    ∀ x : V, x ∉ e → w x = 0 := by
  classical
  cases e using Sym2.ind with
  | _ a b =>
      have hw' : w ∈ edgeWealthChoices (V := V) G a b := by simpa [edgeWealths] using hw
      rcases Finset.mem_image.mp hw' with ⟨k, _hk, rfl⟩
      intro x hx
      have hxne_a : x ≠ a := by
        intro hEq
        exact hx (by simp [hEq])
      have hxne_b : x ≠ b := by
        intro hEq
        exact hx (by simp [hEq])
      simp [edgeWealth, hxne_a, hxne_b]

lemma edgeWealth_conservation {G : ChannelGraph V} {e : Sym2 V} {w : WealthDist V}
    (hND : ¬ e.IsDiag) (hw : w ∈ edgeWealths (V := V) G e) :
    ∀ u v : V, s(u, v) = e → w u + w v = G.cap e := by
  classical
  cases e using Sym2.ind with
  | _ a b =>
      have hab : a ≠ b := by
        have hdiag : ¬ (s(a, b)).IsDiag := by simpa using hND
        simpa [Sym2.mk_isDiag_iff] using hdiag
      have hba : b ≠ a := by simpa [eq_comm] using hab
      have hw' : w ∈ edgeWealthChoices (V := V) G a b := by simpa [edgeWealths] using hw
      rcases Finset.mem_image.mp hw' with ⟨k, hk, rfl⟩
      have hk' : k < G.cap (s(a, b)) + 1 := by simpa [Finset.mem_range] using hk
      have hle : k ≤ G.cap (s(a, b)) := Nat.le_of_lt_succ hk'
      intro u v huv
      have huv' : s(u, v) = s(a, b) := huv
      have huvCases : (u = a ∧ v = b) ∨ (u = b ∧ v = a) := by
        simpa [Sym2.eq_iff] using (show s(u, v) = s(a, b) from huv')
      rcases huvCases with ⟨rfl, rfl⟩ | ⟨rfl, rfl⟩
      · simp [edgeWealth, hba, Nat.add_sub_of_le hle]
      · simp [edgeWealth, hba, Nat.sub_add_cancel hle]

theorem edgeLiquidity_mem_LGOn_singleton {G : ChannelGraph V} {e : Sym2 V} {w : WealthDist V}
    (hND : ¬ e.IsDiag) (hw : w ∈ edgeWealths (V := V) G e) :
    edgeLiquidity (V := V) e w ∈ LiquidityFn.LGOn (V := V) G {e} := by
  classical
  -- unfold the wrapper set so the goals are explicit `∧`-components
  change LiquidityFn.FeasibleOn (V := V) G {e} (edgeLiquidity (V := V) e w)
  unfold LiquidityFn.FeasibleOn
  refine ⟨?_, ?_, ?_⟩
  · intro u v huv
    have huv' : s(u, v) = e := by simpa using (Finset.mem_singleton.mp huv)
    have hsum : w u + w v = G.cap e := edgeWealth_conservation (V := V) (G := G) hND hw u v huv'
    simp [edgeLiquidity, huv', hsum]
  · intro e' x hx
    by_cases he : e' = e
    · subst he
      have : w x = 0 := edgeWealth_support (V := V) (G := G) hw x hx
      simp [edgeLiquidity, this]
    · simp [edgeLiquidity, he]
  · intro e' he x
    have : e' ≠ e := by
      intro hEq
      subst hEq
      exact he (by simp)
    simp [edgeLiquidity, this]
omit [Fintype V] in
theorem piOn_singleton_edgeLiquidity (e : Sym2 V) (w : WealthDist V) :
    piOn (V := V) ({e} : Finset (Sym2 V)) (edgeLiquidity (V := V) e w) = w := by
  classical
  funext x
  simp [piOn, edgeLiquidity]

omit [Fintype V] in
theorem feasibleOn_zeroEdge_of_not_mem {G : ChannelGraph V} {E : Finset (Sym2 V)} {e : Sym2 V} {l : LiquidityFn V}
    (he : e ∉ E) (hl : LiquidityFn.FeasibleOn (V := V) G (insert e E) l) :
    LiquidityFn.FeasibleOn (V := V) G E (zeroEdge (V := V) e l) := by
  classical
  rcases hl with ⟨hCon, hNI, hOG⟩
  refine ⟨?_, ?_, ?_⟩
  · intro u v huv
    have hne : s(u, v) ≠ e := by
      intro hEq
      subst hEq
      exact he huv
    simpa [LiquidityFn.ConservationOn, zeroEdge, hne] using hCon u v (by simp [huv])
  · intro e' x hx
    by_cases hEq : e' = e
    · subst hEq
      simp [zeroEdge]
    · simpa [zeroEdge, hEq] using hNI e' x hx
  · intro e' he' x
    by_cases hEq : e' = e
    · subst hEq
      simp [zeroEdge]
    · have : e' ∉ insert e E := by
        intro hMem
        rcases Finset.mem_insert.mp hMem with hMem | hMem
        · exact hEq hMem
        · exact he' hMem
      simpa [LiquidityFn.OffEdgesZeroOn, zeroEdge, hEq] using hOG e' this x
omit [Fintype V] in
theorem piOn_zeroEdge_of_not_mem {E : Finset (Sym2 V)} {e : Sym2 V} {l : LiquidityFn V}
    (he : e ∉ E) :
    piOn (V := V) E (zeroEdge (V := V) e l) = piOn (V := V) E l := by
  classical
  funext v
  refine Finset.sum_congr rfl ?_
  intro e' he'
  have : e' ≠ e := by
    intro hEq
    subst hEq
    exact he he'
  simp [zeroEdge, this]

theorem mem_WGFinsetOn_iff {G : ChannelGraph V} :
    ∀ (E : Finset (Sym2 V)), E ⊆ G.edges → ∀ (w : WealthDist V),
      w ∈ WGFinsetOn (V := V) G E ↔
        ∃ l : LiquidityFn V, l ∈ LiquidityFn.LGOn (V := V) G E ∧ piOn (V := V) E l = w := by
  intro E
  classical
  refine Finset.induction_on E
    (motive := fun E =>
      ∀ hE : E ⊆ G.edges, ∀ w : WealthDist V,
        w ∈ WGFinsetOn (V := V) G E ↔
          ∃ l : LiquidityFn V, l ∈ LiquidityFn.LGOn (V := V) G E ∧ piOn (V := V) E l = w) ?_ ?_
  · intro _hE w
    constructor
    · intro hw
      have hw' : w = zeroWealth (V := V) := by
        simpa [WGFinsetOn, zeroWealth] using (Finset.mem_singleton.mp (by simpa [WGFinsetOn] using hw))
      subst hw'
      refine ⟨(fun _ _ => 0), ?_, ?_⟩
      · refine ⟨?_, ?_, ?_⟩
        · intro u v huv
          exact (Finset.notMem_empty (s(u, v)) huv).elim
        · intro e x hx
          rfl
        · intro e he x
          rfl
      · exact (piOn_empty (V := V) (l := fun _ _ => 0)).symm
    · rintro ⟨l, _hlE, hpi⟩
      have : piOn (V := V) (∅ : Finset (Sym2 V)) l = zeroWealth (V := V) := piOn_empty (V := V) (l := l)
      have hw : w = zeroWealth (V := V) := by simpa [this] using hpi.symm
      subst hw
      simp [WGFinsetOn]
  · intro e E he ih hInsert w
    have heEdge : e ∈ G.edges := hInsert (by simp)
    have hESub : E ⊆ G.edges := by
      intro e' he'
      exact hInsert (by simp [he'])
    have hND : ¬ e.IsDiag := G.loopless e heEdge
    constructor
    · intro hw
      have hFold :
          WGFinsetOn (V := V) G (insert e E)
            = addWealth (V := V) (edgeWealths (V := V) G e) (WGFinsetOn (V := V) G E) := by
        simp [WGFinsetOn, he]
      have hw' : w ∈ addWealth (V := V) (edgeWealths (V := V) G e) (WGFinsetOn (V := V) G E) := by
        simpa [hFold] using hw
      rcases (mem_addWealth (V := V) (A := edgeWealths (V := V) G e) (B := WGFinsetOn (V := V) G E)
        (w := w)).1 hw' with ⟨w1, hw1, w2, hw2, rfl⟩
      rcases (ih hESub w2).1 hw2 with ⟨l2, hl2E, hpi2⟩
      have hl1 : edgeLiquidity (V := V) e w1 ∈ LiquidityFn.LGOn (V := V) G {e} :=
        edgeLiquidity_mem_LGOn_singleton (V := V) (G := G) hND hw1
      rcases hl1 with ⟨hCon1, hNI1, hOG1⟩
      rcases hl2E with ⟨hCon2, hNI2, hOG2⟩
      refine ⟨edgeLiquidity (V := V) e w1 + l2, ?_, ?_⟩
      · -- feasibility on `insert e E`
        refine ⟨?_, ?_, ?_⟩
        · intro u v huv
          rcases Finset.mem_insert.mp huv with huvEq | huvMem
          · have hsum : w1 u + w1 v = G.cap e :=
              edgeWealth_conservation (V := V) (G := G) hND hw1 u v huvEq
            have h0u : l2 e u = 0 := hOG2 e (by simpa using he) u
            have h0v : l2 e v = 0 := hOG2 e (by simpa using he) v
            simp [huvEq, edgeLiquidity]
            rw [h0u, h0v]
            simp
            exact hsum
          · have hne : s(u, v) ≠ e := by
              intro hEq
              subst hEq
              exact he huvMem
            have h0u : edgeLiquidity (V := V) e w1 (s(u, v)) u = 0 := by
              simp [edgeLiquidity, hne]
            have h0v : edgeLiquidity (V := V) e w1 (s(u, v)) v = 0 := by
              simp [edgeLiquidity, hne]
            have hCon2' := hCon2 u v huvMem
            simp [h0u, h0v]
            exact hCon2'
        · intro e' x hx
          by_cases hEq : e' = e
          · subst e'
            have hwx : w1 x = 0 := edgeWealth_support (V := V) (G := G) hw1 x hx
            have h0 : l2 e x = 0 := hOG2 e (by simpa using he) x
            simp [edgeLiquidity, hwx]
            exact h0
          · have h0 : edgeLiquidity (V := V) e w1 e' x = 0 := by simp [edgeLiquidity, hEq]
            have h2 : l2 e' x = 0 := hNI2 e' x hx
            simp [h0, h2]
        · intro e' he' x
          have hne : e' ≠ e := by
            intro hEq
            subst hEq
            exact he' (by simp)
          have h0a : edgeLiquidity (V := V) e w1 e' x = 0 := by simp [edgeLiquidity, hne]
          have he'E : e' ∉ E := by
            intro hMem
            exact he' (by simp [hMem])
          have h0b : l2 e' x = 0 := hOG2 e' he'E x
          simp [h0a, h0b]
      · -- `piOn` equality
        have h0 : piOn (V := V) E (edgeLiquidity (V := V) e w1) = (0 : WealthDist V) := by
          classical
          funext v
          refine Finset.sum_eq_zero ?_
          intro e' he'
          have hne : e' ≠ e := by
            intro hEq
            subst hEq
            exact he he'
          simp [edgeLiquidity, hne]
        have h1 : piOn (V := V) {e} (edgeLiquidity (V := V) e w1) = w1 :=
          piOn_singleton_edgeLiquidity (V := V) e w1
        have h2 : piOn (V := V) {e} l2 = zeroWealth (V := V) := by
          classical
          funext v
          have : l2 e v = 0 := hOG2 e (by simpa using he) v
          simp [piOn, this, zeroWealth]
        have hInsert :
            piOn (V := V) (insert e E) (edgeLiquidity (V := V) e w1 + l2)
              = w1 + piOn (V := V) E l2 := by
          -- `insert` splits into the singleton edge plus the rest, and the two components are disjoint.
          classical
          have hpiIns := piOn_insert (V := V) (E := E) (e := e) he (l := edgeLiquidity (V := V) e w1 + l2)
          -- rewrite the singleton term
          have hsing :
              (fun v => (edgeLiquidity (V := V) e w1 + l2) e v) = w1 := by
            funext v
            have : l2 e v = 0 := hOG2 e (by simpa using he) v
            simp [edgeLiquidity, this]
          -- and the rest term uses `piOn_add` + the fact that `edgeLiquidity` vanishes on `E`
          calc
            piOn (V := V) (insert e E) (edgeLiquidity (V := V) e w1 + l2)
                = (fun v => (edgeLiquidity (V := V) e w1 + l2) e v) + piOn (V := V) E (edgeLiquidity (V := V) e w1 + l2) := hpiIns
            _   = w1 + (piOn (V := V) E (edgeLiquidity (V := V) e w1) + piOn (V := V) E l2) := by
              simp [hsing, piOn_add (V := V) (E := E) (l₁ := edgeLiquidity (V := V) e w1) (l₂ := l2)]
            _   = w1 + piOn (V := V) E l2 := by
              simp [h0]
        simpa [hpi2] using hInsert
    · rintro ⟨l, hlE, hpi⟩
      rcases hlE with ⟨hCon, hNI, hOG⟩
      let w1 : WealthDist V := piOn (V := V) ({e} : Finset (Sym2 V)) l
      let l2 : LiquidityFn V := zeroEdge (V := V) e l
      have hl2E : LiquidityFn.FeasibleOn (V := V) G E l2 := feasibleOn_zeroEdge_of_not_mem (V := V) (G := G) (E := E) (e := e) he ⟨hCon, hNI, hOG⟩
      have hw2 : piOn (V := V) E l2 = piOn (V := V) E l := piOn_zeroEdge_of_not_mem (V := V) (E := E) (e := e) (l := l) he
      have hw2mem : piOn (V := V) E l2 ∈ WGFinsetOn (V := V) G E := by
        have : ∃ l' : LiquidityFn V, l' ∈ LiquidityFn.LGOn (V := V) G E ∧ piOn (V := V) E l' = piOn (V := V) E l2 :=
          ⟨l2, hl2E, rfl⟩
        exact (ih hESub (piOn (V := V) E l2)).2 this
      have hw1mem : w1 ∈ edgeWealths (V := V) G e := by
        -- unpack `w1` as the single-edge contribution `x ↦ l e x`.
        cases e using Sym2.ind with
        | _ a b =>
            have hConE : l (s(a, b)) a + l (s(a, b)) b = G.cap (s(a, b)) := by
              have : s(a, b) ∈ insert (s(a, b)) E := by simp
              simpa [LiquidityFn.ConservationOn] using hCon a b this
            have hle : l (s(a, b)) a ≤ G.cap (s(a, b)) := by
              have := Nat.le_add_right (l (s(a, b)) a) (l (s(a, b)) b)
              simpa [hConE] using this
            have haRange : l (s(a, b)) a ∈ Finset.range (G.cap (s(a, b)) + 1) := by
              have : l (s(a, b)) a < G.cap (s(a, b)) + 1 := Nat.lt_succ_iff.mpr (by
                have := Nat.le_add_right (l (s(a, b)) a) (l (s(a, b)) b)
                simpa [hConE] using this)
              simpa [Finset.mem_range] using this
            have hw1Eq : w1 = edgeWealth (V := V) G a b (l (s(a, b)) a) := by
              funext x
              by_cases hxa : x = a
              · subst hxa
                simp [w1, piOn, edgeWealth]
              · by_cases hxb : x = b
                · have hba : b ≠ a := by
                    intro hEq
                    apply hxa
                    calc
                      x = b := hxb
                      _ = a := hEq
                  have : l (s(a, b)) b = G.cap (s(a, b)) - l (s(a, b)) a := by
                    have := congrArg (fun t => t - l (s(a, b)) a) hConE
                    simpa [Nat.add_sub_cancel_left] using this
                  simp [w1, piOn, edgeWealth, hxb, hba, this]
                · have hxnot : x ∉ s(a, b) := by
                    simp [Sym2.mem_iff, hxa, hxb]
                  have : l (s(a, b)) x = 0 := hNI (s(a, b)) x hxnot
                  simp [w1, piOn, edgeWealth, hxa, hxb, this]
            -- now show membership in the image
            have : edgeWealth (V := V) G a b (l (s(a, b)) a) ∈ edgeWealthChoices (V := V) G a b := by
              refine Finset.mem_image.mpr ?_
              exact ⟨l (s(a, b)) a, haRange, rfl⟩
            simpa [edgeWealths, w1, hw1Eq] using this
      -- assemble `w ∈ WGFinsetOn (insert e E)` using the fold formula and `addWealth`.
      have hwSum : w1 + piOn (V := V) E l2 = w := by
        -- `piOn` over `insert` splits into the singleton plus `E`.
        have hpiIns :
            piOn (V := V) (insert e E) l = (fun v => l e v) + piOn (V := V) E l :=
          piOn_insert (V := V) (E := E) (e := e) he (l := l)
        -- and `w1 = fun v => l e v`, by `piOn_singleton`.
        have hw1 : w1 = fun v => l e v := by
          funext v
          simp [w1, piOn]
        calc
          w1 + piOn (V := V) E l2
              = (fun v => l e v) + piOn (V := V) E l := by simp [hw1, hw2]
          _ = piOn (V := V) (insert e E) l := by
            simpa using hpiIns.symm
          _ = w := hpi
      have hFold :
          WGFinsetOn (V := V) G (insert e E)
            = addWealth (V := V) (edgeWealths (V := V) G e) (WGFinsetOn (V := V) G E) := by
        simp [WGFinsetOn, he]
      have hwMem : w ∈ addWealth (V := V) (edgeWealths (V := V) G e) (WGFinsetOn (V := V) G E) := by
        refine (mem_addWealth (V := V) (A := edgeWealths (V := V) G e) (B := WGFinsetOn (V := V) G E) (w := w)).2 ?_
        refine ⟨w1, hw1mem, piOn (V := V) E l2, hw2mem, ?_⟩
        simp [hwSum]
      simpa [hFold] using hwMem

theorem wgBool_eq_true_iff {G : ChannelGraph V} {w : WealthDist V} :
    wgBool (V := V) G w = true ↔ w ∈ Wealth.WG (G := G) := by
  classical
  have hLG :
      LiquidityFn.LGOn (V := V) G G.edges =
        HeytingLean.Blockchain.PaymentChannels.LiquidityFn.LG (V := V) G := by
    ext l
    rfl
  have hmem : w ∈ WGFinset (V := V) G ↔ w ∈ Wealth.WG (G := G) := by
    have h := (mem_WGFinsetOn_iff (V := V) (G := G) (E := G.edges) (by simp) (w := w))
    constructor
    · intro hw
      rcases (h.1 hw) with ⟨l, hlOn, hpi⟩
      have hlG : l ∈ HeytingLean.Blockchain.PaymentChannels.LiquidityFn.LG (V := V) G := by
        simpa [hLG] using hlOn
      have hpi' : Wealth.pi G l = w := by
        simpa [Wealth.pi, Wealth.wealth, piOn] using hpi
      exact ⟨l, hlG, hpi'⟩
    · rintro ⟨l, hlG, hpi⟩
      have hlOn : l ∈ LiquidityFn.LGOn (V := V) G G.edges := by
        simpa [hLG] using hlG
      have hpiOn : piOn (V := V) G.edges l = w := by
        simpa [Wealth.pi, Wealth.wealth, piOn] using hpi
      exact (h.2 ⟨l, hlOn, hpiOn⟩)
  simp [wgBool, hmem]

theorem paymentFeasibleBool_eq_true_iff {G : ChannelGraph V} {w : WealthDist V} {i j : V} {a : Cap} :
    paymentFeasibleBool (V := V) G w i j a = true ↔ Payments.PaymentFeasible (V := V) G w i j a := by
  classical
  unfold paymentFeasibleBool Payments.PaymentFeasible
  simp [Bool.and_eq_true, wgBool_eq_true_iff]

end

end Algorithmic

end PaymentChannels
end NucleusDB
