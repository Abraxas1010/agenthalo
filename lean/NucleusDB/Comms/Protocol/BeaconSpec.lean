namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

structure BeaconResponse where
  honest : Bool
  providers : List String
  deriving DecidableEq, Repr

def honestProviderSet (responses : List BeaconResponse) : List String :=
  (responses.foldr
      (fun response acc => if response.honest then response.providers ++ acc else acc)
      []).eraseDups

def providerSupportCount (responses : List BeaconResponse) (provider : String) : Nat :=
  (responses.filter fun response => response.honest && provider ∈ response.providers).length

def crossVerifyBeaconResponses (responses : List BeaconResponse) (quorum : Nat) : List String :=
  (honestProviderSet responses).filter fun provider => quorum ≤ providerSupportCount responses provider

theorem beacon_quorum_censorship_resistant
    (responses : List BeaconResponse)
    (quorum : Nat)
    (provider : String)
    (hMember : provider ∈ crossVerifyBeaconResponses responses quorum) :
    provider ∈ honestProviderSet responses ∧
      quorum ≤ providerSupportCount responses provider := by
  simpa [crossVerifyBeaconResponses] using hMember

theorem quorum_zero_recovers_honest_provider_set
    (responses : List BeaconResponse) :
    crossVerifyBeaconResponses responses 0 = honestProviderSet responses := by
  simp [crossVerifyBeaconResponses]

end Protocol
end Comms
end NucleusDB
end HeytingLean
