import NucleusDB.Comms.AccessControl.PolicyEval
import NucleusDB.Comms.Identity.GenesisDerivation
import NucleusDB.Core.Invariants

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace AccessControl

structure DataState where
  records : List (String × Nat)
  grantCount : Nat
  deriving DecidableEq, Repr

inductive DataDelta where
  | putRecord (key : String) (value : Nat)
  | deleteRecord (key : String)
  | grantAccess (pattern : String)
  deriving DecidableEq, Repr

def deltaRequiredMode : DataDelta → AccessMode
  | .putRecord _ _ => .write
  | .deleteRecord _ => .write
  | .grantAccess _ => .control

def deltaResourceKey : DataDelta → String
  | .putRecord key _ => key
  | .deleteRecord key => key
  | .grantAccess pattern => pattern

def applyDelta : DataState → DataDelta → DataState
  | s, .putRecord key value =>
      { s with records := (key, value) :: s.records.filter (fun p => p.1 != key) }
  | s, .deleteRecord key =>
      { s with records := s.records.filter (fun p => p.1 != key) }
  | s, .grantAccess _ =>
      { s with grantCount := s.grantCount + 1 }

structure AuthChainWitness where
  didValid : Bool
  capabilityValid : Bool
  policyAllows : Bool
  deriving DecidableEq, Repr

def authChainPolicy :
    Core.AuthorizationPolicy DataState DataDelta AuthChainWitness :=
  fun _s _d witness =>
    witness.didValid = true
    ∧ witness.capabilityValid = true
    ∧ witness.policyAllows = true

def noDuplicateKeys (s : DataState) : Prop :=
  s.records.Nodup

/-- Authorized mutation requires all three chain components: DID, capability, and policy. -/
theorem authorized_mutation_requires_full_chain
    (s : DataState) (d : DataDelta) (w : AuthChainWitness)
    (hAuth : authChainPolicy s d w) :
    w.didValid = true ∧ w.capabilityValid = true ∧ w.policyAllows = true := hAuth

/-- putRecord does not change grantCount. -/
theorem putRecord_preserves_grantCount (s : DataState) (k : String) (v : Nat) :
    (applyDelta s (.putRecord k v)).grantCount = s.grantCount := by
  rfl

/-- deleteRecord does not change grantCount. -/
theorem deleteRecord_preserves_grantCount (s : DataState) (k : String) :
    (applyDelta s (.deleteRecord k)).grantCount = s.grantCount := by
  rfl

/-- grantAccess increments grantCount by exactly 1. -/
theorem grantAccess_increments_grantCount (s : DataState) (p : String) :
    (applyDelta s (.grantAccess p)).grantCount = s.grantCount + 1 := by
  rfl

/-- Every delta step preserves or increases grantCount. -/
theorem applyDelta_grantCount_monotone (s : DataState) (d : DataDelta) :
    s.grantCount ≤ (applyDelta s d).grantCount := by
  cases d with
  | putRecord k v => exact Nat.le_refl _
  | deleteRecord k => exact Nat.le_refl _
  | grantAccess p => exact Nat.le_succ _

/-- Replaying a list of deltas monotonically increases grantCount. -/
theorem chain_replay_grantCount_monotone
    (s : DataState) (ds : List DataDelta) :
    s.grantCount ≤ (Core.replay DataState DataDelta applyDelta s ds).grantCount := by
  induction ds generalizing s with
  | nil => exact Nat.le_refl _
  | cons d ds ih =>
    exact Nat.le_trans (applyDelta_grantCount_monotone s d) (ih (applyDelta s d))

theorem broken_chain_rejects_did
    (s : DataState) (d : DataDelta)
    (w : AuthChainWitness) (hBroken : w.didValid = false) :
    ¬ authChainPolicy s d w := by
  intro h
  unfold authChainPolicy at h
  rw [hBroken] at h
  cases h.1

theorem broken_chain_rejects_capability
    (s : DataState) (d : DataDelta)
    (w : AuthChainWitness) (hBroken : w.capabilityValid = false) :
    ¬ authChainPolicy s d w := by
  intro h
  unfold authChainPolicy at h
  rw [hBroken] at h
  cases h.2.1

theorem broken_chain_rejects_policy
    (s : DataState) (d : DataDelta)
    (w : AuthChainWitness) (hBroken : w.policyAllows = false) :
    ¬ authChainPolicy s d w := by
  intro h
  unfold authChainPolicy at h
  rw [hBroken] at h
  cases h.2.2

end AccessControl
end Comms
end NucleusDB
end HeytingLean
