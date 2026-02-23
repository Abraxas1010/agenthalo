import Mathlib.Data.Finset.Lattice.Fold
import NucleusDB.PaymentChannels.EVMAdapter.FromEVMState
import NucleusDB.PaymentChannels.EVMAdapter.State

namespace NucleusDB
namespace PaymentChannels
namespace EVMAdapter

open scoped BigOperators
open Sym2
open NucleusDB.Contracts.Model

/-!
# Blockchain.PaymentChannels.EVMAdapter.Extractor

Concrete extraction of a `ChannelGraph Address` from a concrete `EVMState`.

The extractor reads channel records from a designated channel contract and enumerates channel IDs
via a designated registry contract.
-/

structure ExtractorConfig where
  channelContract : Address
  registryContract : Address
  deriving DecidableEq, Repr

namespace ExtractorConfig

def ids (cfg : ExtractorConfig) (s : EVMState) : Finset Nat :=
  channelIds (state := s) cfg.registryContract

end ExtractorConfig

namespace Extractor

def edgeOfRecord (r : ChannelRecord) : Sym2 Address :=
  s(r.participant1, r.participant2)

def edgeSetOfId (cfg : ExtractorConfig) (s : EVMState) (channelId : Nat) : Finset (Sym2 Address) :=
  match getChannelRecord s cfg.channelContract channelId with
  | none => ∅
  | some r =>
      if r.status = .Open ∧ r.participant1 ≠ r.participant2 then
        { edgeOfRecord r }
      else
        ∅

def extractedEdges (cfg : ExtractorConfig) (s : EVMState) : Finset (Sym2 Address) :=
  (cfg.ids s).biUnion (fun channelId => edgeSetOfId cfg s channelId)

def extractedCap (cfg : ExtractorConfig) (s : EVMState) (e : Sym2 Address) : Cap :=
  (cfg.ids s).sup fun channelId =>
    match getChannelRecord s cfg.channelContract channelId with
    | none => 0
    | some r =>
        if r.status = .Open ∧ edgeOfRecord r = e then r.capacity else 0

/-- Sum capacities over all open channels whose endpoints match `e`.

This is relevant if the settlement layer allows multiple open channels per endpoint-pair.
The current `SettlementSemantics` forbids opening parallel channels, but this extractor supports
hand-constructed states and future extensions. -/
def extractedCapSum (cfg : ExtractorConfig) (s : EVMState) (e : Sym2 Address) : Cap :=
  Finset.sum (cfg.ids s) fun channelId =>
    match getChannelRecord s cfg.channelContract channelId with
    | none => 0
    | some r =>
        if r.status = .Open ∧ edgeOfRecord r = e then r.capacity else 0

theorem extractedEdges_loopless (cfg : ExtractorConfig) (s : EVMState) :
    ∀ e ∈ extractedEdges cfg s, ¬ e.IsDiag := by
  classical
  intro e he
  unfold extractedEdges at he
  rcases (Finset.mem_biUnion).1 he with ⟨channelId, hId, he'⟩
  unfold edgeSetOfId at he'
  cases hRec : getChannelRecord s cfg.channelContract channelId with
  | none =>
      simp [hRec] at he'
  | some r =>
      by_cases hOk : r.status = .Open ∧ r.participant1 ≠ r.participant2
      · simp [hRec, hOk, edgeOfRecord] at he'
        -- `he' : s(r.participant1, r.participant2) = e`
        subst he'
        simpa [Sym2.mk_isDiag_iff] using hOk.2
      · simp [hRec, hOk] at he'

end Extractor

def extractChannelGraph (cfg : ExtractorConfig) (s : EVMState) : ChannelGraph Address := by
  classical
  refine
    { edges := Extractor.extractedEdges cfg s
      cap := Extractor.extractedCap cfg s
      loopless := ?_ }
  intro e he
  exact Extractor.extractedEdges_loopless cfg s e he

/-- Extraction variant that aggregates parallel channels by summing capacities per endpoint-pair.

This agrees with `extractChannelGraph` under the current `SettlementSemantics` (which prevents
opening parallel channels), but it remains useful for experiments on hand-constructed states and
future settlement semantics that permit multiple channels between the same endpoints. -/
def extractChannelGraphSumCap (cfg : ExtractorConfig) (s : EVMState) : ChannelGraph Address := by
  classical
  refine
    { edges := Extractor.extractedEdges cfg s
      cap := Extractor.extractedCapSum cfg s
      loopless := ?_ }
  intro e he
  exact Extractor.extractedEdges_loopless cfg s e he

theorem extractChannelGraph_loopless (cfg : ExtractorConfig) (s : EVMState) :
    ∀ e ∈ (extractChannelGraph cfg s).edges, ¬ e.IsDiag :=
  (extractChannelGraph cfg s).loopless

def concreteAdapter (cfg : ExtractorConfig) : Adapter EVMState :=
  { ChannelGraphOfEVMState := extractChannelGraph cfg }

def concreteAdapterSumCap (cfg : ExtractorConfig) : Adapter EVMState :=
  { ChannelGraphOfEVMState := extractChannelGraphSumCap cfg }

end EVMAdapter
end PaymentChannels
end NucleusDB
