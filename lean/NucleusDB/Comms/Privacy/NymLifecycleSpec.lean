namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Privacy

/-- Nym transport lifecycle states used by policy integration. -/
inductive NymState where
  | disabled
  | starting
  | healthy
  | unhealthy
  deriving DecidableEq, Repr

/-- Operational mode of Nym transport configuration. -/
inductive NymMode where
  | external
  | local
  | disabled
  deriving DecidableEq, Repr

/-- Abstract lifecycle transition function. -/
axiom nymTransition : NymState → NymState

/-- T10: disabled mode is absorbing. -/
axiom nym_disabled_absorbing :
    nymTransition NymState.disabled = NymState.disabled

/-- T10b: healthy state cannot jump directly to disabled. -/
axiom nym_healthy_no_disable :
    nymTransition NymState.healthy = NymState.healthy ∨
    nymTransition NymState.healthy = NymState.unhealthy

end Privacy
end Comms
end NucleusDB
end HeytingLean
