import NucleusDB.PaymentChannels.Liquidity

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators

/-!
# Blockchain.PaymentChannels.Wealth

Wealth distribution induced by a liquidity assignment:

`wealth λ v = ∑ e ∈ G.edges, λ e v`.

The feasible wealth set `WG` is the image of the feasible liquidity set `LG`
under this projection.
-/

universe u

namespace Wealth

variable {V : Type u} [DecidableEq V]

def wealth (G : ChannelGraph V) (l : LiquidityFn V) (v : V) : Cap :=
  ∑ e ∈ G.edges, l e v

def pi (G : ChannelGraph V) (l : LiquidityFn V) : V → Cap :=
  fun v => wealth G l v

def WG (G : ChannelGraph V) : Set (V → Cap) :=
  {w | ∃ l : LiquidityFn V, l ∈ LiquidityFn.LG G ∧ pi G l = w}

end Wealth

end PaymentChannels
end NucleusDB
