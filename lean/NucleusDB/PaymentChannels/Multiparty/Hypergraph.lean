import Mathlib.Data.Finset.Card

namespace NucleusDB
namespace PaymentChannels
namespace Multiparty

/-!
# Blockchain.PaymentChannels.Multiparty.Hypergraph

Minimal data types for multiparty (hyperedge) payment channels.

We model a `k`-party channel as a finite vertex set with `card ≥ 2`, equipped with a capacity.
-/

universe u

/-- A multiparty channel (hyperedge) is a finite set of vertices with at least two members. -/
abbrev Hyperedge (V : Type u) : Type u :=
  {S : Finset V // 2 ≤ S.card}

namespace Hyperedge

variable {V : Type u}

instance : Coe (Hyperedge V) (Finset V) := ⟨Subtype.val⟩

@[simp] lemma coe_mk (S : Finset V) (h : 2 ≤ S.card) :
    ((⟨S, h⟩ : Hyperedge V) : Finset V) = S := rfl

lemma two_le_card (e : Hyperedge V) : 2 ≤ (e : Finset V).card := e.2

end Hyperedge

/-- A finite hypergraph of multiparty channels with per-hyperedge capacities. -/
structure Hypergraph (V : Type u) where
  edges : Finset (Hyperedge V)
  cap : Hyperedge V → Nat

end Multiparty
end PaymentChannels
end NucleusDB
