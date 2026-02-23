import NucleusDB.PaymentChannels.MultiChain.Graph
import NucleusDB.PaymentChannels.Graph

namespace NucleusDB
namespace PaymentChannels
namespace MultiChain

open Sym2

/-!
# NucleusDB.PaymentChannels.MultiChain.Decomposition

Decompose a multi-chain graph into per-chain channel slices and bridge slices.
-/

universe u

variable {V : Type u} [DecidableEq V]

def perChainGraph (G : MultiChainGraph V) (chain : ChainId) : ChannelGraph (ChainAddress V) :=
  { edges := G.edges.filter (fun e => G.edgeType e = .channel chain)
    cap := G.cap
    loopless := by
      intro e he
      exact G.loopless e (Finset.mem_filter.mp he).1 }

def bridgeGraph (G : MultiChainGraph V) (src dst : ChainId) : Finset (Sym2 (ChainAddress V)) :=
  G.edges.filter (fun e => G.edgeType e = .bridge src dst)

theorem mem_perChainGraph_edges_imp_mem_edges
    (G : MultiChainGraph V) (chain : ChainId) {e : Sym2 (ChainAddress V)}
    (he : e ∈ (perChainGraph G chain).edges) :
    e ∈ G.edges :=
  (Finset.mem_filter.mp he).1

theorem mem_bridgeGraph_imp_mem_edges
    (G : MultiChainGraph V) (src dst : ChainId) {e : Sym2 (ChainAddress V)}
    (he : e ∈ bridgeGraph G src dst) :
    e ∈ G.edges :=
  (Finset.mem_filter.mp he).1

theorem multichain_decomposition
    (G : MultiChainGraph V) (e : Sym2 (ChainAddress V)) :
    e ∈ G.edges ↔
      (∃ c : ChainId, e ∈ (perChainGraph G c).edges) ∨
      (∃ src dst : ChainId, e ∈ bridgeGraph G src dst) := by
  constructor
  · intro he
    cases hType : G.edgeType e with
    | channel c =>
        left
        exact ⟨c, Finset.mem_filter.mpr ⟨he, hType⟩⟩
    | bridge src dst =>
        right
        exact ⟨src, dst, Finset.mem_filter.mpr ⟨he, hType⟩⟩
  · intro h
    rcases h with hChan | hBridge
    · rcases hChan with ⟨_c, he⟩
      exact (Finset.mem_filter.mp he).1
    · rcases hBridge with ⟨_src, _dst, he⟩
      exact (Finset.mem_filter.mp he).1

end MultiChain
end PaymentChannels
end NucleusDB
