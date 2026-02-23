import Mathlib.Data.Setoid.Basic
import NucleusDB.PaymentChannels.Wealth

namespace NucleusDB
namespace PaymentChannels

/-!
# Blockchain.PaymentChannels.Quotient

Quotient bridge between feasible liquidity states and feasible wealth distributions.

The core statement is the standard fact: quotienting a type by the kernel of a function `f`
produces a type equivalent to the range of `f`. Here `f` is the wealth projection `π`.
-/

universe u

namespace LiquidityFn

variable {V : Type u} [DecidableEq V]

/-- Feasible liquidity states as a type (subtype of `LiquidityFn V`). -/
abbrev LGType (G : ChannelGraph V) : Type u :=
  {l : LiquidityFn V // l ∈ LG G}

end LiquidityFn

namespace Wealth

variable {V : Type u} [DecidableEq V]

/-- Wealth projection restricted to feasible liquidity states. -/
abbrev piLG (G : ChannelGraph V) : LiquidityFn.LGType (V := V) G → (V → Cap) :=
  fun l => pi G l.1

theorem WG_eq_range_piLG (G : ChannelGraph V) :
    WG (G := G) = Set.range (piLG (V := V) G) := by
  ext w
  constructor
  · intro hw
    rcases hw with ⟨l, hlG, rfl⟩
    refine ⟨⟨l, hlG⟩, rfl⟩
  · rintro ⟨lLG, rfl⟩
    exact ⟨lLG.1, lLG.2, rfl⟩

/-- The quotient `LG / ∼π` (where `λ ∼π μ` iff `π λ = π μ`) is equivalent to `WG`. -/
noncomputable def quotientKer_piLG_equiv_WG (G : ChannelGraph V) :
    Quotient (Setoid.ker (piLG (V := V) G)) ≃ WG (G := G) := by
  classical
  refine (Setoid.quotientKerEquivRange (piLG (V := V) G)).trans ?_
  exact Equiv.setCongr (WG_eq_range_piLG (V := V) G).symm

end Wealth

end PaymentChannels
end NucleusDB
