import Mathlib.Algebra.Order.BigOperators.Group.Finset
import NucleusDB.PaymentChannels.LiquidityNetwork

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators
open Sym2

/-!
# Blockchain.PaymentChannels.NetFlow

Minimal network-flow style notions over an undirected channel graph.

We represent a (directed) flow as a total function `f : V → V → Cap`, but all sums are taken only
over the finite edge set `G.edges : Finset (Sym2 V)`.
-/

universe u

namespace NetFlow

variable {V : Type u} [DecidableEq V]

abbrev Flow (V : Type u) : Type u := V → V → Cap

def edgeOut (x : V) (f : Flow V) : Sym2 V → Cap :=
  Sym2.lift ⟨(fun u v => if x = u then f u v else if x = v then f v u else 0), by
    intro u v
    by_cases hxU : x = u
    · subst hxU
      by_cases hxV : x = v
      · subst hxV
        simp
      · simp [hxV]
    · by_cases hxV : x = v
      · subst hxV
        simp [hxU]
      · simp [hxU, hxV]⟩

def edgeIn (x : V) (f : Flow V) : Sym2 V → Cap :=
  Sym2.lift ⟨(fun u v => if x = u then f v u else if x = v then f u v else 0), by
    intro u v
    by_cases hxU : x = u
    · subst hxU
      by_cases hxV : x = v
      · subst hxV
        simp
      · simp [hxV]
    · by_cases hxV : x = v
      · subst hxV
        simp [hxU]
      · simp [hxU, hxV]⟩

@[simp] lemma edgeOut_mk (x : V) (f : Flow V) (u v : V) :
    edgeOut x f (s(u, v)) = if x = u then f u v else if x = v then f v u else 0 := rfl

@[simp] lemma edgeIn_mk (x : V) (f : Flow V) (u v : V) :
    edgeIn x f (s(u, v)) = if x = u then f v u else if x = v then f u v else 0 := rfl

def outflow (G : ChannelGraph V) (f : Flow V) (x : V) : Cap :=
  ∑ e ∈ G.edges, edgeOut x f e

def inflow (G : ChannelGraph V) (f : Flow V) (x : V) : Cap :=
  ∑ e ∈ G.edges, edgeIn x f e

def Circulation (G : ChannelGraph V) (f : Flow V) : Prop :=
  ∀ x : V, inflow G f x = outflow G f x

def Bounded (G : ChannelGraph V) (l : LiquidityFn V) (f : Flow V) : Prop :=
  ∀ u v : V, f u v ≤ LiquidityNetwork.capDir G l u v

def Strict (G : ChannelGraph V) (f : Flow V) : Prop :=
  ∀ u v : V, s(u, v) ∈ G.edges → f u v = 0 ∨ f v u = 0

theorem bounded_eq_zero_of_not_edge {G : ChannelGraph V} {l : LiquidityFn V} {f : Flow V}
    (hBound : Bounded (G := G) l f) {u v : V} (h : s(u, v) ∉ G.edges) :
    f u v = 0 := by
  have hle : f u v ≤ LiquidityNetwork.capDir G l u v := hBound u v
  have hle0 : f u v ≤ 0 := by simpa [LiquidityNetwork.capDir, h] using hle
  exact Nat.eq_zero_of_le_zero hle0

end NetFlow

end PaymentChannels
end NucleusDB
