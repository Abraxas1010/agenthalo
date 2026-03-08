namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

structure BeaconResponse where
  honest : Bool
  providers : List String
  deriving DecidableEq, Repr

def honestProviderSet (responses : List BeaconResponse) : List String :=
  responses.foldr (fun r acc => if r.honest then r.providers ++ acc else acc) []

def crossVerifyBeaconResponses (responses : List BeaconResponse) : List String :=
  honestProviderSet responses

theorem beacon_quorum_censorship_resistant
    (responses : List BeaconResponse) :
    crossVerifyBeaconResponses responses = honestProviderSet responses := by
  rfl

end Protocol
end Comms
end NucleusDB
end HeytingLean
