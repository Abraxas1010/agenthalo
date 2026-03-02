import NucleusDB.Comms.AccessControl.CapabilityToken

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace AccessControl

inductive AgentMatcher where
  | anyAgent
  | anyAuthenticated
  | byDID (didUris : List String)
  | byTier (minTier : Nat)
  deriving DecidableEq, Repr

structure PolicyRule where
  policyId : String
  matcher : AgentMatcher
  resourcePatterns : List String
  allowedModes : List AccessMode
  deniedModes : List AccessMode
  effectiveFrom : Option Nat
  effectiveUntil : Option Nat
  active : Bool
  deriving DecidableEq, Repr

inductive AccessDecision where
  | allow
  | deny
  | noMatch
  deriving DecidableEq, Repr

def agentMatches (matcher : AgentMatcher) (agentDid : String) (agentTier : Option Nat) : Bool :=
  match matcher with
  | .anyAgent => true
  | .anyAuthenticated => agentDid != ""
  | .byDID didUris => didUris.contains agentDid
  | .byTier minTier =>
      match agentTier with
      | some t => t >= minTier
      | none => false

def PolicyRule.isEffective (rule : PolicyRule) (now : Nat) : Bool :=
  rule.active
    && (match rule.effectiveFrom with | some t => now >= t | none => true)
    && (match rule.effectiveUntil with | some t => now < t | none => true)

def PolicyRule.coversResource (rule : PolicyRule) (key : String) : Bool :=
  rule.resourcePatterns.any (fun p => patternCovers p key)

def applicablePolicies
    (policies : List PolicyRule)
    (agentDid : String)
    (agentTier : Option Nat)
    (key : String)
    (now : Nat) : List PolicyRule :=
  policies.filter (fun rule =>
    rule.isEffective now
      && agentMatches rule.matcher agentDid agentTier
      && rule.coversResource key)

def evaluatePolicies
    (policies : List PolicyRule)
    (agentDid : String)
    (agentTier : Option Nat)
    (key : String)
    (mode : AccessMode)
    (now : Nat) : AccessDecision :=
  let applicable := applicablePolicies policies agentDid agentTier key now
  let denied := applicable.any (fun rule => rule.deniedModes.contains mode)
  let allowed := applicable.any (fun rule => rule.allowedModes.contains mode)
  if denied then
    .deny
  else if allowed then
    .allow
  else
    .noMatch

private theorem allow_result_implies_denied_false
    (denied allowed : Bool)
    (h :
      (if denied then AccessDecision.deny
      else if allowed then AccessDecision.allow
      else AccessDecision.noMatch) = AccessDecision.allow) :
    denied = false := by
  cases denied with
  | true =>
      simp at h
  | false =>
      rfl

private theorem noMatch_result_implies_allowed_false
    (denied allowed : Bool)
    (h :
      (if denied then AccessDecision.deny
      else if allowed then AccessDecision.allow
      else AccessDecision.noMatch) = AccessDecision.noMatch) :
    allowed = false := by
  cases denied with
  | true =>
      cases allowed with
      | true =>
          simp at h
      | false =>
          simp at h
  | false =>
      cases allowed with
      | true =>
          simp at h
      | false =>
          rfl

theorem allow_means_no_deny
    (policies : List PolicyRule)
    (agentDid : String) (agentTier : Option Nat)
    (key : String) (mode : AccessMode) (now : Nat)
    (hEval : evaluatePolicies policies agentDid agentTier key mode now = .allow) :
    (applicablePolicies policies agentDid agentTier key now).any
      (fun rule => rule.deniedModes.contains mode) = false := by
  let denied :=
    (applicablePolicies policies agentDid agentTier key now).any
      (fun rule => rule.deniedModes.contains mode)
  let allowed :=
    (applicablePolicies policies agentDid agentTier key now).any
      (fun rule => rule.allowedModes.contains mode)
  have hShape :
      (if denied then AccessDecision.deny
      else if allowed then AccessDecision.allow
      else AccessDecision.noMatch) = AccessDecision.allow := by
    simpa [evaluatePolicies, denied, allowed] using hEval
  have hDeniedFalse : denied = false := allow_result_implies_denied_false denied allowed hShape
  simpa [denied] using hDeniedFalse

/-- `NoMatch` means default deny: no matching policy allows the requested mode. -/
theorem noMatch_means_no_allow
    (policies : List PolicyRule)
    (agentDid : String) (agentTier : Option Nat)
    (key : String) (mode : AccessMode) (now : Nat)
    (hEval : evaluatePolicies policies agentDid agentTier key mode now = .noMatch) :
    (applicablePolicies policies agentDid agentTier key now).any
      (fun rule => rule.allowedModes.contains mode) = false := by
  let denied :=
    (applicablePolicies policies agentDid agentTier key now).any
      (fun rule => rule.deniedModes.contains mode)
  let allowed :=
    (applicablePolicies policies agentDid agentTier key now).any
      (fun rule => rule.allowedModes.contains mode)
  have hShape :
      (if denied then AccessDecision.deny
      else if allowed then AccessDecision.allow
      else AccessDecision.noMatch) = AccessDecision.noMatch := by
    simpa [evaluatePolicies, denied, allowed] using hEval
  have hAllowedFalse : allowed = false :=
    noMatch_result_implies_allowed_false denied allowed hShape
  simpa [allowed] using hAllowedFalse

end AccessControl
end Comms
end NucleusDB
end HeytingLean
