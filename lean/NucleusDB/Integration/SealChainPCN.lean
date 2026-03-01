import NucleusDB.Integration.PCNToNucleusDB

/-!
# NucleusDB.Integration.SealChainPCN

Seal-chain envelope for committed payment-channel snapshots.
-/

namespace NucleusDB
namespace Integration

universe u

/-- A seal-chain entry carrying a committed PCN payload. -/
structure SealedPCNCommit (V : Type u) [DecidableEq V] where
  payload : PCNCommitPayload V
  sealHash : String
  prevSeal : Option String
  witnessSignature : String
  ctTreeEntry : String

/-- Link predicate between adjacent seal-chain commits. -/
def sealLinked {V : Type u} [DecidableEq V] (prev next : SealedPCNCommit V) : Prop :=
  next.prevSeal = some prev.sealHash

/-- Monotone extension policy for the seal chain over PCN commits. -/
theorem sealed_pcn_monotone_extension {V : Type u} [DecidableEq V]
    (prev next : SealedPCNCommit V) :
    sealLinked prev next →
      next.prevSeal.IsSome
        ∧ (∀ S : Finset V, NucleusDB.PaymentChannels.Cuts.cutIntervalHolds
            (V := V) next.payload.graph next.payload.wealth S) := by
  intro hLink
  refine ⟨?_, ?_⟩
  · exact ⟨prev.sealHash, hLink⟩
  · exact NucleusDB.Integration.committed_pcn_feasible next.payload

end Integration
end NucleusDB
