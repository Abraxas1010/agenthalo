import NucleusDB.Genesis.Entropy.Gate
import Mathlib.Order.Nucleus

namespace HeytingLean
namespace NucleusDB
namespace Genesis

/-- Generative ceremony phases in the noneist/eigenform ontology. -/
inductive CeremonyPhase where
  | void
  | oscillation
  | reEntry
  | nucleus
  deriving DecidableEq, Repr, Inhabited

/-- Canonical linearization of ceremony phases (`void < oscillation < reEntry < nucleus`). -/
def phaseRank : CeremonyPhase → Nat
  | .void => 0
  | .oscillation => 1
  | .reEntry => 2
  | .nucleus => 3

theorem phaseRank_injective : Function.Injective phaseRank := by
  intro a b h
  cases a <;> cases b <;> simp [phaseRank] at h <;> trivial

instance : LinearOrder CeremonyPhase :=
  LinearOrder.lift' phaseRank phaseRank_injective

/-- One-step phase advance. -/
def advance : CeremonyPhase → CeremonyPhase
  | .void => .oscillation
  | .oscillation => .reEntry
  | .reEntry => .nucleus
  | .nucleus => .nucleus

/-- Re-entry nucleus operator (idempotent closure). -/
def R (_ : CeremonyPhase) : CeremonyPhase := .nucleus

theorem advance_reaches_nucleus_in_three :
    advance (advance (advance CeremonyPhase.void)) = CeremonyPhase.nucleus := by
  rfl

theorem nucleus_fixed_point :
    advance CeremonyPhase.nucleus = CeremonyPhase.nucleus := by
  rfl

theorem R_idempotent (p : CeremonyPhase) :
    R (R p) = R p := by
  rfl

/-- `R` is a closure nucleus over the ceremony phase lattice. -/
def R_nucleus : Nucleus CeremonyPhase where
  toFun := R
  map_inf' := by
    intro a b
    simp [R]
  le_apply' := by
    intro a
    cases a <;> native_decide
  idempotent' := by
    intro a
    simp [R]

/-- Gate-aware closure map: transition closes only when policy gate passes. -/
noncomputable def closeIfGate (p : CeremonyPhase) (successes remoteSuccesses : Nat) :
    CeremonyPhase := by
  classical
  exact if Entropy.gateUnlock successes remoteSuccesses then R p else p

/-- Runtime bridge: successful entropy gate actively closes re-entry to nucleus. -/
theorem gate_unlock_closes_reentry
    (successes remoteSuccesses : Nat)
    (h : Entropy.gateUnlock successes remoteSuccesses) :
    closeIfGate CeremonyPhase.reEntry successes remoteSuccesses = CeremonyPhase.nucleus := by
  classical
  simp [closeIfGate, h, R]

/-- Runtime bridge: successful entropy gate corresponds to re-entry closure. -/
theorem gate_unlock_implies_reentry_closure
    (successes remoteSuccesses : Nat)
    (h : Entropy.gateUnlock successes remoteSuccesses) :
    R CeremonyPhase.reEntry = CeremonyPhase.nucleus := by
  have hClosed := gate_unlock_closes_reentry successes remoteSuccesses h
  simpa [closeIfGate, h] using hClosed

end Genesis
end NucleusDB
end HeytingLean
