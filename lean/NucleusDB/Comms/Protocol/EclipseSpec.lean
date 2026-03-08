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

def overlapCount (peerProvided independent : List String) : Nat :=
  (peerProvided.filter fun peer => decide (peer ∈ independent)).length

def verifyTopology (peerProvided independent : List String) : BootstrapConfidence :=
  if independent = [] then
    BootstrapConfidence.unverifiable
  else
    let overlap := overlapCount peerProvided independent
    if overlap = 0 then
      BootstrapConfidence.suspicious
    else if overlap * 2 ≥ independent.length then
      BootstrapConfidence.high
    else
      BootstrapConfidence.moderate

theorem zero_overlap_is_suspicious
    (peerProvided independent : List String)
    (hNonempty : independent ≠ [])
    (hOverlap : overlapCount peerProvided independent = 0) :
    verifyTopology peerProvided independent = BootstrapConfidence.suspicious := by
  simp [verifyTopology, hNonempty, hOverlap]

theorem majority_overlap_is_high
    (peerProvided independent : List String)
    (hNonempty : independent ≠ [])
    (hOverlap : overlapCount peerProvided independent ≠ 0)
    (hMajority : overlapCount peerProvided independent * 2 ≥ independent.length) :
    verifyTopology peerProvided independent = BootstrapConfidence.high := by
  simp [verifyTopology, hNonempty, hOverlap, hMajority]

end Protocol
end Comms
end NucleusDB
end HeytingLean
