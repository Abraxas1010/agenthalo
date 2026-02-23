import NucleusDB.Contracts.Model
import NucleusDB.PaymentChannels.EVMAdapter.FromEVMState
import NucleusDB.PaymentChannels.Payments

namespace NucleusDB
namespace PaymentChannels
namespace EVMAdapter

open Sym2
open NucleusDB.Contracts.Model

/-!
# Blockchain.PaymentChannels.EVMAdapter.SettlementOps

Graph-level model of on-chain settlement operations that affect channel topology/capacities.

This stays deliberately lightweight: the state is a `ChannelGraph Vertex` and calls are
`open/close/splice`.

We also record the *shape* of a seam statement connecting failed off-chain payments to the
necessity of an on-chain topology/capacity change.
-/

namespace SettlementOps

/-- On-chain operation errors (graph-level). -/
inductive Error where
  | loopNotAllowed
  | alreadyOpen
  | missing
  deriving DecidableEq, Repr

/-- On-chain operations that can change channel topology/capacities. -/
inductive Call where
  | open (u v : Vertex) (cap : Cap)
  | close (u v : Vertex)
  | splice (u v : Vertex) (newCap : Cap)
  deriving DecidableEq, Repr

abbrev State := ChannelGraph Vertex

def graphOpen (G : State) (u v : Vertex) (capNew : Cap) : Except Error State := by
  classical
  if huv : u = v then
    exact .error .loopNotAllowed
  else if hMem : s(u, v) ∈ G.edges then
    exact .error .alreadyOpen
  else
    let edges' : Finset (Sym2 Vertex) := insert (s(u, v)) G.edges
    let cap' : Sym2 Vertex → Cap := fun e => if e = s(u, v) then capNew else G.cap e
    have loopless' : ∀ e ∈ edges', ¬ e.IsDiag := by
      intro e he
      rcases (Finset.mem_insert.mp he) with rfl | heOld
      · -- New edge is non-diagonal because `u ≠ v`.
        simpa [Sym2.mk_isDiag_iff] using huv
      · exact G.loopless e heOld
    exact .ok { edges := edges', cap := cap', loopless := loopless' }

def graphClose (G : State) (u v : Vertex) : Except Error State := by
  classical
  if hMem : s(u, v) ∈ G.edges then
    let edges' : Finset (Sym2 Vertex) := G.edges.erase (s(u, v))
    let cap' : Sym2 Vertex → Cap := fun e => if e = s(u, v) then 0 else G.cap e
    have loopless' : ∀ e ∈ edges', ¬ e.IsDiag := by
      intro e he
      exact G.loopless e (Finset.mem_of_mem_erase he)
    exact .ok { edges := edges', cap := cap', loopless := loopless' }
  else
    exact .error .missing

def graphSplice (G : State) (u v : Vertex) (capNew : Cap) : Except Error State := by
  classical
  if hMem : s(u, v) ∈ G.edges then
    let cap' : Sym2 Vertex → Cap := fun e => if e = s(u, v) then capNew else G.cap e
    exact .ok { G with cap := cap' }
  else
    exact .error .missing

def step (G : State) : Call → Except Error State
  | .open u v capNew => graphOpen G u v capNew
  | .close u v => graphClose G u v
  | .splice u v capNew => graphSplice G u v capNew

def init : State :=
  { edges := ∅
    cap := fun _ => 0
    loopless := by
      intro e he _hDiag
      simp at he }

def model : ContractModel State Call Error :=
  { init := init
    step := step }

/-- A seam statement shape: if an off-chain payment is infeasible on the extracted graph,
some on-chain operation must exist that changes the extracted topology/capacities. -/
def NeedsTopologyChange (EVMState : Type) (A : Adapter EVMState)
    (M : ContractModel EVMState Call Error) : Prop :=
  ∀ (s : EVMState) (l : LiquidityFn Vertex) (i j : Vertex) (a : Cap),
    l ∈ LiquidityFn.LG (A.ChannelGraphOfEVMState s) →
    ¬ Payments.PaymentFeasible (A.ChannelGraphOfEVMState s) (Wealth.pi (A.ChannelGraphOfEVMState s) l) i j a →
      ∃ (c : Call) (s' : EVMState),
        M.step s c = .ok s' ∧
        A.ChannelGraphOfEVMState s' ≠ A.ChannelGraphOfEVMState s

end SettlementOps

end EVMAdapter
end PaymentChannels
end NucleusDB
