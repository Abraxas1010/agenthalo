import NucleusDB.Core.Authorization
import NucleusDB.Identity.Delta

namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Authorization witness for identity mutations. -/
structure IdentityAuth where
  actor : String
  authorized : Bool
  provenanceHint : Option String
  deriving DecidableEq, Repr

/-- Delta-local policy obligations. -/
def deltaAuthorized (s : IdentityState) : IdentityDelta → Prop
  | .profileSet name rename =>
      name ≠ "" ∧ (rename = true ∨ s.profileNameLocked = false ∨ s.profileName = none)
  | .anonymousModeSet _ => True
  | .securityTierSet _ => True
  | .deviceSet device => device.enabled = true ∧ device.entropyBits > 0
  | .networkSet network => networkConfigured network = true ∨ s.anonymousMode = true

/-- Identity authorization policy: caller must be explicitly authorized,
    have a non-empty actor id, and satisfy delta-local constraints. -/
def identityPolicy :
    Core.AuthorizationPolicy IdentityState IdentityDelta IdentityAuth :=
  fun s d auth => auth.authorized = true ∧ auth.actor.length > 0 ∧ deltaAuthorized s d

theorem identityPolicy_rejects_unauthorized
    (s : IdentityState) (d : IdentityDelta) (actor : String) :
    ¬ identityPolicy s d { actor := actor, authorized := false, provenanceHint := none } := by
  simp [identityPolicy]

theorem identityPolicy_requires_actor
    (s : IdentityState) (d : IdentityDelta) (authorized : Bool) :
    ¬ identityPolicy s d { actor := "", authorized := authorized, provenanceHint := none } := by
  simp [identityPolicy]

theorem identityPolicy_accepts_profile_set_example
    (s : IdentityState) (name actor : String)
    (hName : name ≠ "")
    (hActor : actor.length > 0) :
    identityPolicy s (.profileSet name true)
      { actor := actor, authorized := true, provenanceHint := none } := by
  simp [identityPolicy, deltaAuthorized, hName, hActor]

end Identity
end NucleusDB
end HeytingLean
