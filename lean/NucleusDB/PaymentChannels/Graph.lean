import NucleusDB.PaymentChannels.Basic
import Mathlib.Data.Finset.Sym

namespace NucleusDB
namespace PaymentChannels

open Sym2

/-!
# Blockchain.PaymentChannels.Graph

A simple undirected channel graph with per-channel capacities.

We represent undirected edges as `Sym2 V` (unordered pairs). The `cap` function is total;
only values on `edges` are intended to matter.
-/

universe u

structure ChannelGraph (V : Type u) [DecidableEq V] where
  edges : Finset (Sym2 V)
  cap : Sym2 V → Cap
  loopless : ∀ e ∈ edges, ¬ e.IsDiag

namespace ChannelGraph

variable {V : Type u} [DecidableEq V] (G : ChannelGraph V)

lemma ne_of_mem_edges {u v : V} (h : s(u, v) ∈ G.edges) : u ≠ v := by
  have hdiag : ¬ (s(u, v)).IsDiag := G.loopless (s(u, v)) h
  simpa [Sym2.mk_isDiag_iff] using hdiag

end ChannelGraph

end PaymentChannels
end NucleusDB

