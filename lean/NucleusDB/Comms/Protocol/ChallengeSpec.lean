namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

inductive ChallengeOutcome
  | pass
  | fail
  deriving DecidableEq, Repr

structure ChallengeSpec where
  challengeId : String
  passed : ChallengeOutcome
  deriving DecidableEq, Repr

def responseAccepts (c : ChallengeSpec) : Prop :=
  c.passed = ChallengeOutcome.pass

theorem accepted_response_is_not_failed (c : ChallengeSpec)
    (h : responseAccepts c) : c.passed ≠ ChallengeOutcome.fail := by
  intro hFail
  unfold responseAccepts at h
  rw [hFail] at h
  cases h

end Protocol
end Comms
end NucleusDB
end HeytingLean
