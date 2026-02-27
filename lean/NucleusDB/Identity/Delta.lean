import NucleusDB.Identity.State

namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Identity state transition language mirrored by runtime mutation endpoints. -/
inductive IdentityDelta where
  | profileSet (name : String) (rename : Bool)
  | anonymousModeSet (enabled : Bool)
  | securityTierSet (tier : IdentitySecurityTier)
  | deviceSet (device : DeviceIdentity)
  | networkSet (network : NetworkIdentity)
  deriving DecidableEq, Repr

/-- Deterministic transition function for identity mutations. -/
def applyDelta (s : IdentityState) : IdentityDelta → IdentityState
  | .profileSet name rename =>
      let nextRevision :=
        if rename && s.profileName.isSome then s.profileNameRevision + 1 else s.profileNameRevision
      { s with
        profileName := some name
        profileNameLocked := true
        profileNameRevision := nextRevision
      }
  | .anonymousModeSet enabled =>
      if enabled then
        { s with anonymousMode := true, device := none, network := none }
      else
        { s with anonymousMode := false }
  | .securityTierSet tier => { s with securityTier := some tier }
  | .deviceSet device => { s with device := some device }
  | .networkSet network => { s with network := some network }

theorem applyDelta_profile_locks_name
    (s : IdentityState) (name : String) (rename : Bool) :
    (applyDelta s (.profileSet name rename)).profileNameLocked = true := by
  simp [applyDelta]

theorem applyDelta_anonymous_clears_network
    (s : IdentityState) :
    (applyDelta s (.anonymousModeSet true)).network = none := by
  simp [applyDelta]

theorem applyDelta_securityTier_set
    (s : IdentityState) (tier : IdentitySecurityTier) :
    (applyDelta s (.securityTierSet tier)).securityTier = some tier := by
  simp [applyDelta]

end Identity
end NucleusDB
end HeytingLean
