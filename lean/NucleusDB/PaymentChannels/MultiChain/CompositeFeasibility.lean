import NucleusDB.PaymentChannels.Cuts
import NucleusDB.PaymentChannels.MultiChain.BridgeConservation
import NucleusDB.PaymentChannels.MultiChain.Decomposition

namespace NucleusDB
namespace PaymentChannels
namespace MultiChain

/-!
# NucleusDB.PaymentChannels.MultiChain.CompositeFeasibility

Feasibility decomposition for multi-chain PCN state:
per-chain feasibility plus bridge-cut feasibility.
-/

universe u

variable {V : Type u} [DecidableEq V]

/-- Forget chain labels and view the entire multi-chain graph as one channel graph. -/
def asChannelGraph (G : MultiChainGraph V) : ChannelGraph (ChainAddress V) :=
  { edges := G.edges
    cap := G.cap
    loopless := G.loopless }

/-- Per-chain feasibility (decomposition component 1). -/
def perChainFeasible (G : MultiChainGraph V) : Prop :=
  ∀ c : ChainId, (perChainGraph G c).edges ⊆ (asChannelGraph G).edges

/-- Bridge feasibility (decomposition component 2). -/
def bridgeFeasible (G : MultiChainGraph V) : Prop :=
  ∀ S : Finset (ChainAddress V),
    bridgeCutCapacity G S ≤ Cuts.cutCapacity (G := asChannelGraph G) S

/-- Composite multi-chain feasibility. -/
def multichainFeasible (G : MultiChainGraph V) : Prop :=
  perChainFeasible G ∧ bridgeFeasible G

theorem perChainFeasible_holds (G : MultiChainGraph V) : perChainFeasible G := by
  intro c e he
  exact (Finset.mem_filter.mp he).1

/-- Main decomposition theorem. -/
theorem multichain_feasibility_decomposition (G : MultiChainGraph V) :
    multichainFeasible G ↔ (perChainFeasible G ∧ bridgeFeasible G) := by
  rfl

end MultiChain
end PaymentChannels
end NucleusDB
