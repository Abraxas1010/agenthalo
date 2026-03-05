import NucleusDB.Comms.Mesh.MeshSpec

/-!
# NucleusDB.Comms.Mesh.ConsensusProperties

Consensus algebra for mesh height reconciliation.

These properties justify the monotone height checks used by the runtime mesh
coordinator (`src/container/mesh.rs`) during peer synchronization.
-/

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Mesh

/-- Height reconciliation step for mesh consensus. -/
def meshConsensusStep (localHeight peerHeight : Nat) : Nat :=
  Nat.max localHeight peerHeight

/-- Termination/idempotence: reapplying the same peer vote is stable. -/
theorem mesh_consensus_idempotent (localHeight peerHeight : Nat) :
    meshConsensusStep (meshConsensusStep localHeight peerHeight) peerHeight =
      meshConsensusStep localHeight peerHeight := by
  unfold meshConsensusStep
  exact Nat.max_eq_left (Nat.le_max_right localHeight peerHeight)

/-- Safety: local state never decreases. -/
theorem mesh_consensus_safe (localHeight peerHeight : Nat) :
    localHeight ≤ meshConsensusStep localHeight peerHeight := by
  exact Nat.le_max_left localHeight peerHeight

/-- Validity: peer contribution is always represented in the next state. -/
theorem mesh_consensus_valid (localHeight peerHeight : Nat) :
    peerHeight ≤ meshConsensusStep localHeight peerHeight := by
  exact Nat.le_max_right localHeight peerHeight

/-- Consensus merge is commutative. -/
theorem mesh_consensus_commutative (a b : Nat) :
    meshConsensusStep a b = meshConsensusStep b a := by
  simp [meshConsensusStep, Nat.max_comm]

/-- Join characterization: `max` is exactly the least upper bound. -/
theorem mesh_consensus_join_characterization (a b z : Nat) :
    meshConsensusStep a b ≤ z ↔ a ≤ z ∧ b ≤ z := by
  constructor
  · intro h
    exact ⟨Nat.le_trans (Nat.le_max_left a b) h, Nat.le_trans (Nat.le_max_right a b) h⟩
  · intro h
    exact (Nat.max_le).2 h

end Mesh
end Comms
end NucleusDB
end HeytingLean
