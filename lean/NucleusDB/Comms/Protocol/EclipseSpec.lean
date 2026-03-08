namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

inductive BootstrapConfidence
  | high
  | moderate
  | suspicious
  | unverifiable
  deriving DecidableEq, Repr

def verifyTopology (peerProvided independent : List String) : BootstrapConfidence :=
  if independent = [] then
    BootstrapConfidence.unverifiable
  else if peerProvided.any (fun p => decide (p ∈ independent)) then
    BootstrapConfidence.moderate
  else
    BootstrapConfidence.suspicious

theorem eclipse_detected_on_zero_overlap
    (peerProvided independent : List String)
    (hNonempty : independent ≠ [])
    (hDisjoint : ∀ p, p ∈ peerProvided → p ∉ independent) :
    verifyTopology peerProvided independent = BootstrapConfidence.suspicious := by
  unfold verifyTopology
  rw [if_neg hNonempty]
  have hAnyFalse : List.any peerProvided (fun p => decide (p ∈ independent)) = false := by
    induction peerProvided with
    | nil => rfl
    | cons hd tl ih =>
      have hHd : hd ∉ independent := hDisjoint hd (by simp)
      have hTl : ∀ p, p ∈ tl → p ∉ independent := by
        intro p hp
        exact hDisjoint p (List.mem_cons_of_mem _ hp)
      simp [hHd, ih hTl]
  simp [hAnyFalse]

end Protocol
end Comms
end NucleusDB
end HeytingLean
