import NucleusDB.Genesis.Entropy.State

namespace HeytingLean
namespace NucleusDB
namespace Genesis
namespace Entropy

structure HarvestPolicy where
  minSources : Nat
  requireRemote : Bool
  deriving DecidableEq, Repr

def defaultPolicy : HarvestPolicy where
  minSources := sourceMinSuccess
  requireRemote := true

def policyPass (policy : HarvestPolicy) (successes remoteSuccesses : Nat) : Prop :=
  successes ≥ policy.minSources ∧ (¬ policy.requireRemote ∨ remoteSuccesses > 0)

def gateUnlock (successes remoteSuccesses : Nat) : Prop :=
  policyPass defaultPolicy successes remoteSuccesses

theorem policyPass_implies_minSources
    (policy : HarvestPolicy) {successes remoteSuccesses : Nat}
    (h : policyPass policy successes remoteSuccesses) :
    successes ≥ policy.minSources := by
  exact h.1

theorem gate_unlock_requires_remote
    {successes remoteSuccesses : Nat}
    (h : gateUnlock successes remoteSuccesses) :
    remoteSuccesses > 0 := by
  exact (h.2.resolve_left (by decide))

theorem gateUnlock_equiv_policy :
    gateUnlock = policyPass defaultPolicy := by
  rfl

end Entropy
end Genesis
end NucleusDB
end HeytingLean

