import Mathlib.Data.Sym.Sym2
import NucleusDB.Contracts.Model
import NucleusDB.PaymentChannels.EVMAdapter.Extractor
import NucleusDB.PaymentChannels.MultiChain.Graph

namespace NucleusDB
namespace PaymentChannels
namespace MultiChain
namespace EVMAdapter

open scoped BigOperators
open Sym2
open NucleusDB.Contracts.Model
open NucleusDB.PaymentChannels.EVMAdapter

/-!
# NucleusDB.PaymentChannels.MultiChain.EVMAdapter.MultiChainExtractor

Extraction wrapper for per-chain EVM states into a unified multi-chain graph surface.
-/

structure MultiChainEVMState where
  perChainState : ChainId → PaymentChannels.EVMAdapter.EVMState
  bridgeContract : ChainId → Option Address

structure MultiChainExtractorConfig where
  chains : Finset ChainId
  perChain : ChainId → PaymentChannels.EVMAdapter.ExtractorConfig


def embedAddr (chain : ChainId) : Address → ChainAddress Address :=
  fun a => { chain := chain, address := a }

lemma embedAddr_injective (chain : ChainId) : Function.Injective (embedAddr chain) := by
  intro a b h
  exact congrArg ChainAddress.address h

def perChainGraph (cfg : MultiChainExtractorConfig) (st : MultiChainEVMState) (chain : ChainId) :
    ChannelGraph Address :=
  PaymentChannels.EVMAdapter.extractChannelGraph (cfg.perChain chain) (st.perChainState chain)

/-- Lift each per-chain edge set into a chain-tagged edge universe. -/
def extractedEdges (cfg : MultiChainExtractorConfig) (st : MultiChainEVMState) :
    Finset (Sym2 (ChainAddress Address)) :=
  cfg.chains.biUnion fun chain =>
    ((perChainGraph cfg st chain).edges).image (Sym2.map (embedAddr chain))

/-- Aggregate capacities from per-chain extractor outputs. -/
def extractedCap (cfg : MultiChainExtractorConfig) (st : MultiChainEVMState)
    (e : Sym2 (ChainAddress Address)) : Cap :=
  ∑ chain ∈ cfg.chains,
    ∑ e0 ∈ (perChainGraph cfg st chain).edges,
      if Sym2.map (embedAddr chain) e0 = e then (perChainGraph cfg st chain).cap e0 else 0

lemma extractedEdges_loopless (cfg : MultiChainExtractorConfig) (st : MultiChainEVMState) :
    ∀ e ∈ extractedEdges cfg st, ¬ e.IsDiag := by
  intro e he
  rcases Finset.mem_biUnion.mp he with ⟨chain, hChain, hEdge⟩
  rcases Finset.mem_image.mp hEdge with ⟨e0, he0, rfl⟩
  have hLoop : ¬ e0.IsDiag := (perChainGraph cfg st chain).loopless e0 he0
  have hIff : (Sym2.map (embedAddr chain) e0).IsDiag ↔ e0.IsDiag :=
    Sym2.isDiag_map (e := e0) (embedAddr_injective chain)
  intro hDiag
  exact hLoop (hIff.mp hDiag)

/-- Current extraction emits only per-chain channels; bridge discovery is modeled separately. -/
def extractedEdgeType (_cfg : MultiChainExtractorConfig) (_st : MultiChainEVMState)
    (_e : Sym2 (ChainAddress Address)) : EdgeType :=
  .bridge .base .ethereum

/-- Unified multi-chain graph extracted from chain-local EVM snapshots. -/
def extractMultiChainGraph (cfg : MultiChainExtractorConfig) (st : MultiChainEVMState) :
    MultiChainGraph Address :=
  { edges := extractedEdges cfg st
    edgeType := extractedEdgeType cfg st
    cap := extractedCap cfg st
    loopless := extractedEdges_loopless cfg st
    bridge_connects_different := by
      intro e he
      simp [extractedEdgeType]
    channel_same_chain := by
      intro e he
      simp [extractedEdgeType] }

end EVMAdapter
end MultiChain
end PaymentChannels
end NucleusDB
