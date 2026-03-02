import NucleusDB.Comms.Identity.GenesisDerivation
import NucleusDB.Comms.Identity.DIDKeySpec
import NucleusDB.Core.Authorization

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace AccessControl

inductive AccessMode where
  | read
  | write
  | append
  | control
  deriving DecidableEq, Repr

inductive AgentClass where
  | publicAgent
  | authenticated
  | verified (minTier : Nat)
  | specific (didUri : String)
  deriving DecidableEq, Repr

structure TimeBounds where
  notBefore : Nat
  expiresAt : Nat
  deriving DecidableEq, Repr

structure CapabilityToken where
  grantorDid : String
  granteeDid : String
  agentClass : AgentClass
  resourcePatterns : List String
  modes : List AccessMode
  timeBounds : TimeBounds
  delegatable : Bool
  signatureValid : Bool
  revoked : Bool
  deriving DecidableEq, Repr

def CapabilityToken.isValid (token : CapabilityToken) (now : Nat) : Prop :=
  token.signatureValid = true
    ∧ token.revoked = false
    ∧ now ≥ token.timeBounds.notBefore
    ∧ now < token.timeBounds.expiresAt

def patternCovers (pattern : String) (key : String) : Bool :=
  pattern == "*" || pattern == key

def CapabilityToken.authorizes
    (token : CapabilityToken) (key : String) (mode : AccessMode) (now : Nat) : Prop :=
  token.isValid now
    ∧ mode ∈ token.modes
    ∧ token.resourcePatterns.any (fun p => patternCovers p key) = true

theorem capability_requires_valid_signature
    (token : CapabilityToken) (now : Nat)
    (hValid : token.isValid now) :
    token.signatureValid = true := by
  exact hValid.1

theorem expired_token_cannot_authorize
    (token : CapabilityToken) (key : String) (mode : AccessMode) (now : Nat)
    (hExpired : now ≥ token.timeBounds.expiresAt) :
    ¬ token.authorizes key mode now := by
  intro hAuth
  exact Nat.not_lt_of_ge hExpired hAuth.1.2.2.2

theorem revoked_token_cannot_authorize
    (token : CapabilityToken) (key : String) (mode : AccessMode) (now : Nat)
    (hRevoked : token.revoked = true) :
    ¬ token.authorizes key mode now := by
  intro hAuth
  have hNotRevoked : token.revoked = false := hAuth.1.2.1
  rw [hRevoked] at hNotRevoked
  cases hNotRevoked

end AccessControl
end Comms
end NucleusDB
end HeytingLean
