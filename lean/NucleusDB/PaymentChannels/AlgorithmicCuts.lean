import Mathlib.Data.Fintype.Powerset
import NucleusDB.PaymentChannels.Payments

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators

/-!
# Blockchain.PaymentChannels.AlgorithmicCuts

Phase 5: a **sound cut-based certificate** for infeasibility.

If a wealth distribution `w` violates the cut-interval inequalities for some subset `S`,
then `w ∉ WG`.  For finite vertex types we can search for such an `S` and return it as an
explicit witness.
-/

namespace Cuts

universe u

variable {V : Type u} [DecidableEq V]

def wealthSum (w : V → Cap) (S : Finset V) : Cap :=
  ∑ x ∈ S, w x

def cutIntervalHolds (G : ChannelGraph V) (w : V → Cap) (S : Finset V) : Prop :=
  internalCapacity G S ≤ wealthSum (V := V) w S ∧
    wealthSum (V := V) w S ≤ internalCapacity G S + cutCapacity G S

def cutViolation (G : ChannelGraph V) (w : V → Cap) (S : Finset V) : Prop :=
  ¬ cutIntervalHolds (V := V) G w S

instance (G : ChannelGraph V) (w : V → Cap) (S : Finset V) :
    Decidable (cutIntervalHolds (V := V) G w S) := by
  unfold cutIntervalHolds
  infer_instance

instance (G : ChannelGraph V) (w : V → Cap) (S : Finset V) :
    Decidable (cutViolation (V := V) G w S) := by
  unfold cutViolation
  infer_instance

theorem cutIntervalHolds_of_mem_WG {G : ChannelGraph V} {w : V → Cap}
    (hw : w ∈ Wealth.WG (G := G)) (S : Finset V) :
    cutIntervalHolds (V := V) G w S := by
  rcases hw with ⟨l, hlG, rfl⟩
  simpa [cutIntervalHolds, wealthSum, wealthIn] using (cutInterval (G := G) (l := l) hlG S)

theorem not_mem_WG_of_cutViolation {G : ChannelGraph V} {w : V → Cap} {S : Finset V}
    (hV : cutViolation (V := V) G w S) :
    w ∉ Wealth.WG (G := G) := by
  intro hw
  exact hV (cutIntervalHolds_of_mem_WG (V := V) (G := G) (w := w) hw S)

theorem not_paymentFeasible_of_cutViolation {G : ChannelGraph V} {w : V → Cap} {i j : V} {a : Cap} {S : Finset V}
    (hV : cutViolation (V := V) G (Payments.pay w i j a) S) :
    ¬ Payments.PaymentFeasible (V := V) G w i j a := by
  intro hPF
  exact not_mem_WG_of_cutViolation (V := V) (G := G) (w := Payments.pay w i j a) (S := S) hV hPF.2

namespace Algorithmic

variable [Fintype V]

def violatingCuts (G : ChannelGraph V) (w : V → Cap) : Finset (Finset V) :=
  (Finset.univ : Finset (Finset V)).filter (fun S => decide (cutViolation (V := V) G w S))

def cutObstructedBool (G : ChannelGraph V) (w : V → Cap) : Bool :=
  decide (violatingCuts (V := V) G w).Nonempty

theorem mem_violatingCuts_iff {G : ChannelGraph V} {w : V → Cap} {S : Finset V} :
    S ∈ violatingCuts (V := V) G w ↔ cutViolation (V := V) G w S := by
  classical
  unfold violatingCuts
  simp

theorem cutObstructedBool_eq_true_iff {G : ChannelGraph V} {w : V → Cap} :
    cutObstructedBool (V := V) G w = true ↔ ∃ S : Finset V, cutViolation (V := V) G w S := by
  classical
  unfold cutObstructedBool
  constructor
  · intro h
    have hNE : (violatingCuts (V := V) G w).Nonempty := by
      simpa using (decide_eq_true_iff.mp h)
    rcases hNE with ⟨S, hS⟩
    exact ⟨S, (mem_violatingCuts_iff (V := V) (G := G) (w := w)).1 hS⟩
  · rintro ⟨S, hS⟩
    have : (violatingCuts (V := V) G w).Nonempty := by
      refine ⟨S, ?_⟩
      exact (mem_violatingCuts_iff (V := V) (G := G) (w := w)).2 hS
    exact (decide_eq_true_iff.mpr this)

theorem cutObstructedBool_not_mem_WG {G : ChannelGraph V} {w : V → Cap}
    (h : cutObstructedBool (V := V) G w = true) :
    w ∉ Wealth.WG (G := G) := by
  classical
  rcases (cutObstructedBool_eq_true_iff (V := V) (G := G) (w := w)).1 h with ⟨S, hS⟩
  exact not_mem_WG_of_cutViolation (V := V) (G := G) (w := w) (S := S) hS

theorem cutObstructedBool_not_paymentFeasible {G : ChannelGraph V} {w : V → Cap} {i j : V} {a : Cap}
    (h : cutObstructedBool (V := V) G (Payments.pay w i j a) = true) :
    ¬ Payments.PaymentFeasible (V := V) G w i j a := by
  classical
  rcases (cutObstructedBool_eq_true_iff (V := V) (G := G) (w := Payments.pay w i j a)).1 h with ⟨S, hS⟩
  exact not_paymentFeasible_of_cutViolation (V := V) (G := G) (w := w) (i := i) (j := j) (a := a) (S := S) hS

end Algorithmic

end Cuts

end PaymentChannels
end NucleusDB
