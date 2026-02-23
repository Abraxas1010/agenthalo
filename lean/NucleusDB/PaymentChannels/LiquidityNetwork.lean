import NucleusDB.PaymentChannels.Liquidity

namespace NucleusDB
namespace PaymentChannels

open Sym2

/-!
# Blockchain.PaymentChannels.LiquidityNetwork

The directed liquidity network induced by an undirected channel graph `G` and a liquidity state `λ`.

For an undirected channel `{u,v}`, we expose directed capacities:
- `capDir u v = λ({u,v}, u)`
- `capDir v u = λ({u,v}, v)`
- otherwise `0`.
-/

universe u

namespace LiquidityNetwork

variable {V : Type u} [DecidableEq V]

def capDir (G : ChannelGraph V) (l : LiquidityFn V) (u v : V) : Cap :=
  if s(u, v) ∈ G.edges then
    l (s(u, v)) u
  else
    0

theorem capDir_add_capDir_eq_cap {G : ChannelGraph V} {l : LiquidityFn V}
    (hLG : l ∈ LiquidityFn.LG G) {u v : V} (hEdge : s(u, v) ∈ G.edges) :
    capDir G l u v + capDir G l v u = G.cap (s(u, v)) := by
  classical
  rcases hLG with ⟨hCon, _hNI, _hOG⟩
  have hEdge' : s(v, u) ∈ G.edges := by simpa [Sym2.eq_swap] using hEdge
  have hv :
      l (s(v, u)) v = l (s(u, v)) v := by
    simpa using congrArg (fun e => l e v) (Sym2.eq_swap (a := v) (b := u))
  -- Expand `capDir` in both directions.
  simp [capDir, hEdge, hEdge', hv, hCon u v hEdge]

end LiquidityNetwork

end PaymentChannels
end NucleusDB
