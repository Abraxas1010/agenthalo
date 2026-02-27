import NucleusDB.Identity.State
import NucleusDB.Sheaf.MaterializationFunctor

namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Stable identity POD key index (subset for formal materialization model). -/
inductive IdentityPodKey where
  | profileHasName
  | anonymousMode
  | deviceEnabled
  | networkConfigured
  deriving DecidableEq, Repr

def profileHasNameFlag (s : IdentityState) : Bool :=
  match s.profileName with
  | some name =>
      if name.isEmpty then false else true
  | none => false

def deviceEnabledFlag (s : IdentityState) : Bool :=
  s.device.map (fun d => d.enabled) |>.getD false

def networkConfiguredFlag (s : IdentityState) : Bool :=
  s.network.map networkConfigured |>.getD false

/-- Coarse string-valued materialization to POD key-value payloads. -/
def materializeIdentity (s : IdentityState) : IdentityPodKey → String
  | .profileHasName => toString (profileHasNameFlag s)
  | .anonymousMode => toString s.anonymousMode
  | .deviceEnabled => toString (deviceEnabledFlag s)
  | .networkConfigured => toString (networkConfiguredFlag s)

/-- Transport relation preserving the POD-visible projection while allowing
    internal bookkeeping to vary. -/
def identityTransports (s t : IdentityState) : Prop :=
  profileHasNameFlag s = profileHasNameFlag t
    ∧ s.anonymousMode = t.anonymousMode
    ∧ deviceEnabledFlag s = deviceEnabledFlag t
    ∧ networkConfiguredFlag s = networkConfiguredFlag t

/-- Identity state materialization as a sheaf-compatible functor. -/
def identityMaterializationFunctor :
    Sheaf.MaterializationFunctor IdentityState IdentityPodKey String where
  toVector := materializeIdentity
  transports := identityTransports
  naturality := by
    intro s t h
    rcases h with ⟨hProfile, hAnon, hDevice, hNet⟩
    funext k
    cases k <;> simp [materializeIdentity, hProfile, hAnon, hDevice, hNet]

theorem identityMaterialization_transport_eq
    {s t : IdentityState}
    (h : identityTransports s t) :
    identityMaterializationFunctor.toVector s =
      identityMaterializationFunctor.toVector t := by
  exact identityMaterializationFunctor.naturality s t h

end Identity
end NucleusDB
end HeytingLean
