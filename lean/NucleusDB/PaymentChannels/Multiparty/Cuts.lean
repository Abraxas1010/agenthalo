import Mathlib.Algebra.Order.BigOperators.Group.Finset
import Mathlib.Data.Finset.SDiff
import NucleusDB.PaymentChannels.Multiparty.Hypergraph

namespace NucleusDB
namespace PaymentChannels
namespace Multiparty

open scoped BigOperators

/-!
# Blockchain.PaymentChannels.Multiparty.Cuts

Cut-style capacity constraints for multiparty channels (hyperedges).

A hyperedge is said to *straddle* a cut `S` if it has at least one vertex in `S` and at least one
vertex outside `S`.
-/

universe u

namespace Cuts

variable {V : Type u} [DecidableEq V]

/-- A hyperedge straddles `S` if it meets both `S` and the complement side. -/
def Straddles (e : Hyperedge V) (S : Finset V) : Prop :=
  ((e : Finset V) ∩ S).Nonempty ∧ ((e : Finset V) \ S).Nonempty

instance (e : Hyperedge V) (S : Finset V) : Decidable (Straddles (e := e) S) := by
  classical
  unfold Straddles
  infer_instance

/-- Cut capacity: sum of capacities of hyperedges that straddle `S`. -/
def cutCapacity (H : Hypergraph V) (S : Finset V) : Nat :=
  ∑ e ∈ H.edges, if Straddles e S then H.cap e else 0

namespace Examples

inductive Node
  | A | B | C
  deriving DecidableEq, Repr

open Node

def eABC : Hyperedge Node :=
  ⟨({A, B, C} : Finset Node), by decide⟩

def toyHypergraph : Hypergraph Node :=
  { edges := ({eABC} : Finset (Hyperedge Node))
    cap := fun _ => 10 }

example : cutCapacity (H := toyHypergraph) ({A} : Finset Node) = 10 := by
  have hStr : Straddles eABC ({A} : Finset Node) := by decide
  simp [cutCapacity, toyHypergraph, hStr]

end Examples

end Cuts

end Multiparty
end PaymentChannels
end NucleusDB
