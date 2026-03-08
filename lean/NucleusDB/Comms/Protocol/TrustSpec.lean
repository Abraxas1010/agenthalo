namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

inductive ChallengeDifficulty
  | ping
  | standard
  | deep
  deriving DecidableEq, Repr

structure VerificationRecord where
  difficulty : ChallengeDifficulty
  passed : Bool
  deriving DecidableEq, Repr

def difficultyWeight : ChallengeDifficulty → Nat
  | .ping => 1
  | .standard => 10
  | .deep => 50

def trustContribution (r : VerificationRecord) : Int :=
  let weight := Int.ofNat (difficultyWeight r.difficulty)
  if r.passed then weight else -(2 * weight)

def rawTrust (records : List VerificationRecord) : Int :=
  records.foldl (fun acc r => acc + trustContribution r) 0

def computeTrust (records : List VerificationRecord) : Int :=
  if rawTrust records < 0 then 0 else rawTrust records

theorem trust_floor_nonneg (records : List VerificationRecord) :
    0 ≤ computeTrust records := by
  unfold computeTrust
  by_cases h : rawTrust records < 0
  · simp [h]
  · simp [h]
    exact Int.not_lt.mp h

theorem ping_insufficient_for_routing (r : VerificationRecord)
    (hDiff : r.difficulty = ChallengeDifficulty.ping)
    (hPass : r.passed = true) :
    computeTrust [r] < 5 := by
  cases r with
  | mk difficulty passed =>
      cases difficulty <;> cases passed <;> simp at hDiff hPass
      simp [computeTrust, rawTrust, trustContribution, difficultyWeight]

end Protocol
end Comms
end NucleusDB
end HeytingLean
