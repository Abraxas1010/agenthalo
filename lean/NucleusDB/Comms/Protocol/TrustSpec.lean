import Mathlib

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
  verifiedAt : Nat
  deriving DecidableEq, Repr

def difficultyWeight : ChallengeDifficulty → Nat
  | .ping => 1
  | .standard => 10
  | .deep => 50

def decayExponent (now halfLife verifiedAt : Nat) : Nat :=
  if halfLife = 0 then 0 else (now - verifiedAt) / halfLife

def decayedWeight (difficulty : ChallengeDifficulty) (now halfLife verifiedAt : Nat) : Nat :=
  if halfLife = 0 then 0 else difficultyWeight difficulty / 2 ^ decayExponent now halfLife verifiedAt

def trustContribution (r : VerificationRecord) (now halfLife : Nat) : Int :=
  let weight := Int.ofNat (decayedWeight r.difficulty now halfLife r.verifiedAt)
  if r.passed then weight else -(2 * weight)

def rawTrust (records : List VerificationRecord) (now halfLife : Nat) : Int :=
  records.foldl (fun acc r => acc + trustContribution r now halfLife) 0

def computeTrust (records : List VerificationRecord) (now halfLife : Nat) : Int :=
  if rawTrust records now halfLife < 0 then 0 else rawTrust records now halfLife

/-- `computeTrust` is tracked in deci-points so it can model the Rust `f64`
runtime exactly at one decimal place without introducing floating-point proof
obligations. Divide by `10` to compare against the Rust thresholds. -/
def runtimeTrustApprox (records : List VerificationRecord) (now halfLife : Nat) : Rat :=
  computeTrust records now halfLife / 10

def sybilCost (identities challengeCost : Nat) : Nat :=
  identities * challengeCost

def slash (trust penalty : Nat) : Nat :=
  trust - penalty

theorem trust_floor_nonneg (records : List VerificationRecord) (now halfLife : Nat) :
    0 ≤ computeTrust records now halfLife := by
  unfold computeTrust
  by_cases h : rawTrust records now halfLife < 0
  · simp [h]
  · simp [h]
    exact Int.not_lt.mp h

theorem ping_insufficient_for_routing :
    runtimeTrustApprox [{ difficulty := ChallengeDifficulty.ping, passed := true, verifiedAt := 0 }] 0 1 < (1 : Rat) / 2 := by
  native_decide

theorem trust_decays_to_zero (difficulty : ChallengeDifficulty) :
    ∃ age, decayedWeight difficulty age 1 0 = 0 := by
  cases difficulty with
  | ping =>
      refine ⟨1, ?_⟩
      native_decide
  | standard =>
      refine ⟨4, ?_⟩
      native_decide
  | deep =>
      refine ⟨6, ?_⟩
      native_decide

theorem sybil_cost_linear (n₁ n₂ challengeCost : Nat) :
    sybilCost (n₁ + n₂) challengeCost = sybilCost n₁ challengeCost + sybilCost n₂ challengeCost := by
  simp [sybilCost, Nat.add_mul]

theorem slashing_punishes_fraud
    (trust penalty : Nat)
    (hPenalty : 0 < penalty)
    (hPenaltyLe : penalty ≤ trust) :
    slash trust penalty < trust := by
  unfold slash
  omega

end Protocol
end Comms
end NucleusDB
end HeytingLean
