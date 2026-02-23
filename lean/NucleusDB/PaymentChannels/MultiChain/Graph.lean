import Mathlib.Data.Sym.Sym2
import NucleusDB.PaymentChannels.Basic
import NucleusDB.PaymentChannels.Graph

namespace NucleusDB
namespace PaymentChannels
namespace MultiChain

open Sym2

/-!
# NucleusDB.PaymentChannels.MultiChain.Graph

Core multi-chain graph types used by the NucleusDB PCN trust layer.
-/

universe u

inductive ChainId
  | base
  | ethereum
  | arbitrum
  | optimism
  | custom (name : String)
  deriving DecidableEq, Repr

structure ChainAddress (V : Type u) where
  chain : ChainId
  address : V
  deriving DecidableEq, Repr

inductive EdgeType
  | channel (chain : ChainId)
  | bridge (left right : ChainId)
  deriving DecidableEq, Repr

structure MultiChainGraph (V : Type u) [DecidableEq V] where
  edges : Finset (Sym2 (ChainAddress V))
  edgeType : Sym2 (ChainAddress V) → EdgeType
  cap : Sym2 (ChainAddress V) → Cap
  loopless : ∀ e ∈ edges, ¬ e.IsDiag
  bridge_connects_different :
    ∀ e ∈ edges,
      match edgeType e with
      | .bridge left right => left ≠ right
      | .channel _ => True
  channel_same_chain :
    ∀ e ∈ edges,
      match edgeType e with
      | .channel c => ∀ a ∈ e.toFinset, a.chain = c
      | .bridge _ _ => True

end MultiChain
end PaymentChannels
end NucleusDB
