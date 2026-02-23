import NucleusDB.Core.Invariants
import NucleusDB.PaymentChannels.CutCompleteness
import NucleusDB.PaymentChannels.Graph
import NucleusDB.PaymentChannels.Liquidity
import NucleusDB.PaymentChannels.Rebalancing
import NucleusDB.PaymentChannels.Wealth

/-!
# NucleusDB.Integration.PCNToNucleusDB

Bridge layer connecting copied payment-channel proofs to NucleusDB commit payloads.
-/

namespace NucleusDB
namespace Integration

open NucleusDB.PaymentChannels

universe u

/-- A payment-channel snapshot suitable for commitment into NucleusDB. -/
structure PCNCommitPayload (V : Type u) [DecidableEq V] where
  graph : ChannelGraph V
  liquidity : LiquidityFn V
  liquidityFeasible : liquidity ∈ LiquidityFn.LG graph
  wealth : V → Cap
  wealthWitness : wealth = Wealth.pi graph liquidity
  wealthProof : wealth ∈ Wealth.WG (G := graph)
  seqCounters : V → Nat

/-- Every committed PCN state satisfies all cut-interval constraints. -/
theorem committed_pcn_feasible {V : Type u} [DecidableEq V]
    (payload : PCNCommitPayload V) :
    ∀ S : Finset V, Cuts.cutIntervalHolds (V := V) payload.graph payload.wealth S := by
  exact (Cuts.mem_WG_iff_forall_cutIntervalHolds (G := payload.graph) (w := payload.wealth)).mp
    payload.wealthProof

/-- A rebalancing transition between two committed PCN snapshots. -/
structure PCNTransition (V : Type u) [DecidableEq V] where
  before : PCNCommitPayload V
  after : PCNCommitPayload V
  graphEq : after.graph = before.graph
  wealthEq : after.wealth = before.wealth

/-- Rebalancing transitions preserve committed wealth vectors. -/
theorem rebalance_commit_preserves_wealth {V : Type u} [DecidableEq V]
    (t : PCNTransition V) :
    t.after.wealth = t.before.wealth := by
  simpa using t.wealthEq

end Integration
end NucleusDB
