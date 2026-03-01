import Mathlib.Order.Nucleus
import NucleusDB.Genesis.Noneist

namespace HeytingLean
namespace NucleusDB
namespace Genesis

/-- Lawvere-style local operator induced by the ceremony nucleus. -/
abbrev ceremonyClosure : ClosureOperator CeremonyPhase :=
  R_nucleus.toClosureOperator

/-- Fixed points of `R` are exactly the terminal nucleus phase. -/
theorem ceremony_fixed_iff_nucleus (p : CeremonyPhase) :
    R p = p ↔ p = CeremonyPhase.nucleus := by
  cases p <;> simp [R]

/-- The terminal ceremony phase is closed under the induced closure operator. -/
theorem nucleus_is_closed : ClosureOperator.IsClosed ceremonyClosure CeremonyPhase.nucleus := by
  refine (ceremonyClosure.isClosed_iff).2 ?_
  rfl

/-- Unlocking the gate yields a closed phase via the gate-aware closure map. -/
theorem gate_unlock_yields_closed
    (successes remoteSuccesses : Nat)
    (h : Entropy.gateUnlock successes remoteSuccesses) :
    ClosureOperator.IsClosed ceremonyClosure
      (closeIfGate CeremonyPhase.reEntry successes remoteSuccesses) := by
  have hClose : closeIfGate CeremonyPhase.reEntry successes remoteSuccesses = CeremonyPhase.nucleus :=
    gate_unlock_closes_reentry successes remoteSuccesses h
  simpa [hClose] using nucleus_is_closed

end Genesis
end NucleusDB
end HeytingLean
