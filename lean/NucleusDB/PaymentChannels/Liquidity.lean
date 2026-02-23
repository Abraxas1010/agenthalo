import NucleusDB.PaymentChannels.Graph

namespace NucleusDB
namespace PaymentChannels

open Sym2

/-!
# Blockchain.PaymentChannels.Liquidity

Liquidity states `λ` assign per-channel balances to endpoints, subject to:
- conservation on each channel edge (`λ(e,u) + λ(e,v) = cap(e)`), and
- zeros off the graph and off the channel endpoints.

This file defines the feasible liquidity set `LG`.
-/

universe u

abbrev LiquidityFn (V : Type u) : Type u := Sym2 V → V → Cap

namespace LiquidityFn

variable {V : Type u} [DecidableEq V]

def Conservation (G : ChannelGraph V) (l : LiquidityFn V) : Prop :=
  ∀ u v : V,
    s(u, v) ∈ G.edges →
      l (s(u, v)) u + l (s(u, v)) v = G.cap (s(u, v))

def NonIncidentZero (l : LiquidityFn V) : Prop :=
  ∀ e : Sym2 V, ∀ x : V, x ∉ e → l e x = 0

def OffGraphZero (G : ChannelGraph V) (l : LiquidityFn V) : Prop :=
  ∀ e : Sym2 V, e ∉ G.edges → ∀ x : V, l e x = 0

def Feasible (G : ChannelGraph V) (l : LiquidityFn V) : Prop :=
  Conservation G l ∧ NonIncidentZero l ∧ OffGraphZero G l

def LG (G : ChannelGraph V) : Set (LiquidityFn V) :=
  {l | Feasible G l}

theorem le_cap_of_mem_LG {G : ChannelGraph V} {l : LiquidityFn V}
    (hLG : l ∈ LG G) {u v : V} (hEdge : s(u, v) ∈ G.edges) :
    l (s(u, v)) u ≤ G.cap (s(u, v)) := by
  rcases hLG with ⟨hCon, _hNI, _hOG⟩
  have hEq := hCon u v hEdge
  calc
    l (s(u, v)) u ≤ l (s(u, v)) u + l (s(u, v)) v := by
      exact Nat.le_add_right _ _
    _ = G.cap (s(u, v)) := hEq

end LiquidityFn

end PaymentChannels
end NucleusDB
