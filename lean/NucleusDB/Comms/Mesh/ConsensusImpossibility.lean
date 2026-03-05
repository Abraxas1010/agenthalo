import Mathlib.Data.Finset.Basic
import NucleusDB.Comms.Mesh.MeshSpec

/-!
# NucleusDB.Comms.Mesh.ConsensusImpossibility

Async mesh-consensus impossibility surface for blackout schedules.

Transferred from the NucleusPOD FLP blackout family, adapted to NucleusDB's
`PeerId` mesh model.
-/

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Mesh

/-- Delivery oracle for async mesh communication. -/
abbrev MeshSchedule : Type := PeerId → PeerId → Bool

/-- Async run model over mesh participants. -/
structure MeshAsyncRun where
  delivered : MeshSchedule
  crashed : PeerId → Bool
  participants : Finset PeerId

/-- Consensus decision rule evaluated per peer on an async run. -/
abbrev MeshDecisionRule : Type := MeshAsyncRun → PeerId → Bool

/-- Total blackout: no packet is delivered between any peer pair. -/
def meshBlackout (r : MeshAsyncRun) : Prop :=
  ∀ src dst, r.delivered src dst = false

/-- A rule is blackout-sound if it never decides `true` during blackout. -/
def meshBlackoutSound (rule : MeshDecisionRule) : Prop :=
  ∀ r, meshBlackout r → ∀ p, rule r p = false

/-- FLP-style blackout consequence: no participant can terminate positively. -/
theorem mesh_blackout_no_termination
    (rule : MeshDecisionRule)
    (hSound : meshBlackoutSound rule)
    (r : MeshAsyncRun)
    (hBlackout : meshBlackout r)
    (p : PeerId) :
    rule r p = false := by
  exact hSound r hBlackout p

/-- Blackout corollary restricted to registered participants. -/
theorem mesh_blackout_no_participant_decision
    (rule : MeshDecisionRule)
    (hSound : meshBlackoutSound rule)
    (r : MeshAsyncRun)
    (hBlackout : meshBlackout r)
    (p : PeerId)
    (hp : p ∈ r.participants) :
    rule r p = false := by
  have _ := hp
  exact mesh_blackout_no_termination rule hSound r hBlackout p

/-- Under blackout, a nonempty participant set cannot all decide `true`. -/
theorem mesh_blackout_no_global_termination
    (rule : MeshDecisionRule)
    (hSound : meshBlackoutSound rule)
    (r : MeshAsyncRun)
    (hBlackout : meshBlackout r)
    (hNonempty : r.participants.Nonempty) :
    ¬ (∀ p ∈ r.participants, rule r p = true) := by
  intro hAll
  rcases hNonempty with ⟨p, hp⟩
  have hFalse : rule r p = false :=
    mesh_blackout_no_participant_decision rule hSound r hBlackout p hp
  have hTrue : rule r p = true := hAll p hp
  have hContra : false = true := hFalse.symm.trans hTrue
  exact Bool.false_ne_true hContra

/-- Repair-channel decision wrapper: consensus bit or external repair witness. -/
def meshRepairDecide (consensusBit repair : Bool) : Bool :=
  consensusBit || repair

/-- External repair escapes blackout impossibility exactly when repair is `true`. -/
theorem mesh_nucleus_escapes_flp (repair : Bool) :
    meshRepairDecide false repair = true ↔ repair = true := by
  cases repair <;> decide

end Mesh
end Comms
end NucleusDB
end HeytingLean
