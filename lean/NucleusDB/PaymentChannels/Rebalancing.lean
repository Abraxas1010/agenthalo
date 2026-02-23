import NucleusDB.PaymentChannels.NetFlow
import NucleusDB.PaymentChannels.Wealth

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators
open Sym2

/-!
# Blockchain.PaymentChannels.Rebalancing

Given a feasible liquidity state `l ∈ LG G` and a bounded circulation `f`, we can update the
per-edge endpoint balances to obtain a new feasible liquidity state with the same wealth
projection `π`.

This is the basic formal bridge from “rebalancing” to “circulation on the induced directed
liquidity network”.
-/

universe u

namespace Rebalancing

variable {V : Type u} [DecidableEq V]

open NetFlow

def apply (G : ChannelGraph V) (l : LiquidityFn V) (f : Flow V) : LiquidityFn V :=
  fun e x =>
    Sym2.lift ⟨(fun u v =>
      if hEdge : s(u, v) ∈ G.edges then
        if x = u then
          (l (s(u, v)) u - f u v) + f v u
        else if x = v then
          (l (s(u, v)) v - f v u) + f u v
        else
          0
      else
        0), by
      intro u v
      by_cases hEdge : s(u, v) ∈ G.edges
      · have hEdge' : s(v, u) ∈ G.edges := by simpa [Sym2.eq_swap] using hEdge
        by_cases hxu : x = u
        · subst hxu
          by_cases hxv : x = v
          · subst hxv
            simp [hEdge]
          · simp [hEdge, Sym2.eq_swap, hxv]
        · by_cases hxv : x = v
          · subst hxv
            simp [hEdge', Sym2.eq_swap, hxu]
          · simp [hEdge, Sym2.eq_swap, hxu, hxv]
      · have hEdge' : ¬ s(v, u) ∈ G.edges := by simpa [Sym2.eq_swap] using hEdge
        simp [hEdge, hEdge']⟩ e

@[simp] lemma apply_mk (G : ChannelGraph V) (l : LiquidityFn V) (f : Flow V) (u v x : V) :
    apply G l f (s(u, v)) x =
      if _hEdge : s(u, v) ∈ G.edges then
        if x = u then
          (l (s(u, v)) u - f u v) + f v u
        else if x = v then
          (l (s(u, v)) v - f v u) + f u v
        else
          0
      else
        0 := rfl

theorem apply_mem_LG {G : ChannelGraph V} {l : LiquidityFn V} (hLG : l ∈ LiquidityFn.LG G)
    {f : Flow V} (hBound : Bounded (G := G) l f) :
    apply G l f ∈ LiquidityFn.LG G := by
  classical
  rcases hLG with ⟨hCon, hNI, _hOG⟩
  refine ⟨?_, ?_, ?_⟩
  · -- Conservation
    intro u v hEdge
    have huv : u ≠ v := G.ne_of_mem_edges (u := u) (v := v) hEdge
    have huv_le : f u v ≤ l (s(u, v)) u := by
      have hle : f u v ≤ LiquidityNetwork.capDir G l u v := hBound u v
      simpa [LiquidityNetwork.capDir, hEdge] using hle
    have hvu_le : f v u ≤ l (s(u, v)) v := by
      have hle : f v u ≤ LiquidityNetwork.capDir G l v u := hBound v u
      simpa [LiquidityNetwork.capDir, hEdge, Sym2.eq_swap] using hle
    have hConUV : l (s(u, v)) u + l (s(u, v)) v = G.cap (s(u, v)) := hCon u v hEdge
    calc
      apply G l f (s(u, v)) u + apply G l f (s(u, v)) v
          = ((l (s(u, v)) u - f u v) + f v u) + ((l (s(u, v)) v - f v u) + f u v) := by
              simp [apply, hEdge, huv.symm]
      _   = ((l (s(u, v)) u - f u v) + f u v) + ((l (s(u, v)) v - f v u) + f v u) := by
              simp [add_assoc, add_left_comm, add_comm]
      _   = l (s(u, v)) u + l (s(u, v)) v := by
              simp [Nat.sub_add_cancel huv_le, Nat.sub_add_cancel hvu_le]
      _   = G.cap (s(u, v)) := hConUV
  · -- NonIncidentZero
    intro e x hx
    cases e using Sym2.ind with
    | _ u v =>
        have hxne_u : x ≠ u := by
          intro hEq
          exact hx (by simp [hEq])
        have hxne_v : x ≠ v := by
          intro hEq
          exact hx (by simp [hEq])
        by_cases hEdge : s(u, v) ∈ G.edges <;> simp [apply, hEdge, hxne_u, hxne_v]
  · -- OffGraphZero
    intro e he x
    cases e using Sym2.ind with
    | _ u v =>
        have : s(u, v) ∉ G.edges := by simpa using he
        simp [apply, this]

theorem pi_eq_of_circulation {G : ChannelGraph V} {l : LiquidityFn V} (hLG : l ∈ LiquidityFn.LG G)
    {f : Flow V} (hBound : Bounded (G := G) l f) (hCirc : Circulation (G := G) f) :
    Wealth.pi G (apply G l f) = Wealth.pi G l := by
  classical
  rcases hLG with ⟨_hCon, hNI, _hOG⟩
  funext x
  -- Expand the updated wealth as (old - out) + in.
  have hTerm :
      ∀ e ∈ G.edges,
        apply G l f e x = (l e x - edgeOut x f e) + edgeIn x f e := by
    intro e he
    cases e using Sym2.ind with
    | _ u v =>
        by_cases hxu : x = u
        · simp [apply, he, edgeOut, edgeIn, hxu]
        · by_cases hxv : x = v
          ·
            have hvu : v ≠ u := (G.ne_of_mem_edges (u := u) (v := v) he).symm
            simp [apply, he, edgeOut, edgeIn, hxv, hvu]
          · have hxnot : x ∉ s(u, v) := by
              simp [Sym2.mem_iff, hxu, hxv]
            have hlx : l (s(u, v)) x = 0 := hNI (s(u, v)) x hxnot
            simp [apply, he, edgeOut, edgeIn, hxu, hxv, hlx]

  have hOut_le :
      ∀ e ∈ G.edges, edgeOut x f e ≤ l e x := by
    intro e he
    cases e using Sym2.ind with
    | _ u v =>
        by_cases hxu : x = u
        · have hle : f u v ≤ LiquidityNetwork.capDir G l u v := hBound u v
          have hle' : f u v ≤ l (s(u, v)) u := by simpa [LiquidityNetwork.capDir, he] using hle
          simpa [edgeOut, hxu] using hle'
        · by_cases hxv : x = v
          · have hvu : v ≠ u := (G.ne_of_mem_edges (u := u) (v := v) he).symm
            have hle : f v u ≤ LiquidityNetwork.capDir G l v u := hBound v u
            have hEdge' : s(v, u) ∈ G.edges := by simpa [Sym2.eq_swap] using he
            have hle' : f v u ≤ l (s(v, u)) v := by
              simpa [LiquidityNetwork.capDir, hEdge'] using hle
            have hv : l (s(v, u)) v = l (s(u, v)) v := by
              simpa using congrArg (fun e => l e v) (Sym2.eq_swap (a := v) (b := u))
            have hle'' : f v u ≤ l (s(u, v)) v := by simpa [hv] using hle'
            simpa [edgeOut, hvu, hxv] using hle''
          · have hxnot : x ∉ s(u, v) := by
              simp [Sym2.mem_iff, hxu, hxv]
            have hlx : l (s(u, v)) x = 0 := hNI (s(u, v)) x hxnot
            simp [edgeOut, hxu, hxv, hlx]

  have hIn_eq_out :
      (∑ e ∈ G.edges, edgeIn x f e) = ∑ e ∈ G.edges, edgeOut x f e := by
    simpa [inflow, outflow] using hCirc x

  have hCancel :
      (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeOut x f e)
        = ∑ e ∈ G.edges, l e x := by
    -- Turn the LHS into a single sum and use `Nat.sub_add_cancel` termwise.
    have hAsSum :
        (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeOut x f e)
          = ∑ e ∈ G.edges, ((l e x - edgeOut x f e) + edgeOut x f e) := by
      simpa using
        (Finset.sum_add_distrib (s := G.edges)
          (f := fun e => l e x - edgeOut x f e)
          (g := fun e => edgeOut x f e)).symm
    -- Discharge the termwise cancellations.
    calc
      (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeOut x f e)
          = ∑ e ∈ G.edges, ((l e x - edgeOut x f e) + edgeOut x f e) := hAsSum
      _   = ∑ e ∈ G.edges, l e x := by
              refine Finset.sum_congr rfl ?_
              intro e he
              simpa using Nat.sub_add_cancel (hOut_le e he)

  -- Now compute `wealth` and use circulation to cancel in/out.
  have hWealth :
      Wealth.wealth G (apply G l f) x
        = (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeIn x f e) := by
    -- Expand and rewrite each term with `hTerm`, then distribute.
    calc
      Wealth.wealth G (apply G l f) x
          = ∑ e ∈ G.edges, apply G l f e x := rfl
      _   = ∑ e ∈ G.edges, ((l e x - edgeOut x f e) + edgeIn x f e) := by
              refine Finset.sum_congr rfl ?_
              intro e he
              simp [hTerm e he]
      _   = (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeIn x f e) := by
              simpa using
                (Finset.sum_add_distrib (s := G.edges)
                  (f := fun e => l e x - edgeOut x f e)
                  (g := fun e => edgeIn x f e))

  -- Replace the inflow sum with the outflow sum using circulation.
  calc
    Wealth.wealth G (apply G l f) x
        = (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeIn x f e) := hWealth
    _   = (∑ e ∈ G.edges, (l e x - edgeOut x f e)) + (∑ e ∈ G.edges, edgeOut x f e) := by
            simp [hIn_eq_out]
    _   = ∑ e ∈ G.edges, l e x := hCancel
    _   = Wealth.wealth G l x := rfl

end Rebalancing

end PaymentChannels
end NucleusDB
