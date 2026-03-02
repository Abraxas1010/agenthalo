import NucleusDB.Comms.Privacy.RouterSpec

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Privacy

/-- Outbound gate model: maximum privacy traffic needs proxy or explicit fail-open. -/
def routeAllowed (proxyHealthy : Bool) (failOpen : Bool) (level : PrivacyLevel) : Prop :=
  match level with
  | .maximum => proxyHealthy = true ∨ failOpen = true
  | .p2p => True
  | .none => True

theorem maximum_without_proxy_blocks_when_fail_closed
    (proxyHealthy failOpen : Bool)
    (hProxy : proxyHealthy = false) (hFailOpen : failOpen = false) :
    ¬ routeAllowed proxyHealthy failOpen .maximum := by
  unfold routeAllowed
  intro h
  rcases h with hOk | hOpen
  · simp [hProxy] at hOk
  · simp [hFailOpen] at hOpen

theorem maximum_allows_with_healthy_proxy
    (proxyHealthy failOpen : Bool)
    (hProxy : proxyHealthy = true) :
    routeAllowed proxyHealthy failOpen .maximum := by
  unfold routeAllowed
  exact Or.inl hProxy

/-- Message types that must route via mixnet under default policy. -/
inductive SensitiveDidcommType where
  | taskSend
  | taskStatus
  | taskArtifact
  | taskCancel
  | credentialOffer
  | credentialRequest
  | credentialIssue
  deriving DecidableEq, Repr

/-- Policy-level statement: sensitive DIDComm traffic is classified as maximum privacy. -/
def sensitiveDidcommRoutesMaximum (_ : SensitiveDidcommType) : PrivacyLevel :=
  .maximum

theorem sensitive_didcomm_types_route_via_maximum
    (t : SensitiveDidcommType) :
    sensitiveDidcommRoutesMaximum t = .maximum := by
  rfl

end Privacy
end Comms
end NucleusDB
end HeytingLean
