import Std
import Mathlib.Data.Finset.Sort
import Mathlib.Data.Prod.Lex
import Mathlib.Data.Sym.Sym2.Order
import NucleusDB.PaymentChannels.AlgorithmicCuts

namespace NucleusDB
namespace PaymentChannels

open scoped BigOperators
open Sym2

/-!
# Blockchain.PaymentChannels.AlgorithmicFlow

Phase 6: a polynomial-time checker for membership in the feasible wealth set `WG`.

The earlier `Algorithmic.wgBool` checker is intentionally exponential (it enumerates all edge splits).
This module complements it with a max-flow reduction:

- create one node per channel edge and one node per vertex;
- send `cap(e)` from `source → e`;
- allow each edge-node `e` to send up to `cap(e)` to each of its two endpoints;
- require each vertex-node `v` to send exactly `w(v)` to `sink`.

If such a flow exists, we can read off a liquidity assignment (`l e v` is the flow from the edge-node
`e` to the vertex-node `v`).

Implementation note: we use a bounded Edmonds–Karp (BFS augmenting paths) over an adjacency-matrix
residual graph. This is meant as a practical executable checker; it does not replace the existing
proof development around cuts/Hall.
-/

namespace AlgorithmicFlow

universe u

variable {V : Type u} [DecidableEq V] [Fintype V] [LinearOrder V]

abbrev Matrix := Array (Array Cap)

private def totalCap (G : ChannelGraph V) : Cap :=
  ∑ e ∈ G.edges, G.cap e

private def totalWealth (w : V → Cap) : Cap :=
  ∑ v : V, w v

private def sym2Key (e : Sym2 V) : V ×ₗ V :=
  toLex (e.inf, e.sup)

private def sym2Le (a b : Sym2 V) : Prop :=
  sym2Key (V := V) a ≤ sym2Key (V := V) b

private instance : DecidableRel (sym2Le (V := V)) := by
  intro a b
  unfold sym2Le
  infer_instance

private instance : IsTrans (Sym2 V) (sym2Le (V := V)) :=
  ⟨by
    intro a b c hab hbc
    exact le_trans hab hbc⟩

private instance : IsTotal (Sym2 V) (sym2Le (V := V)) :=
  ⟨by
    intro a b
    exact le_total (sym2Key (V := V) a) (sym2Key (V := V) b)⟩

private instance : IsAntisymm (Sym2 V) (sym2Le (V := V)) :=
  ⟨by
    intro a b hab hba
    have hKey : sym2Key (V := V) a = sym2Key (V := V) b := le_antisymm hab hba
    have hInf : a.inf = b.inf := by
      simpa [sym2Key] using congrArg (fun p : V × V => p.1) (congrArg ofLex hKey)
    have hSup : a.sup = b.sup := by
      simpa [sym2Key] using congrArg (fun p : V × V => p.2) (congrArg ofLex hKey)
    exact (Sym2.inf_eq_inf_and_sup_eq_sup (α := V)).1 ⟨hInf, hSup⟩⟩

private def findIdxList? {α : Type u} [DecidableEq α] (xs : List α) (a : α) : Option Nat :=
  let rec go (i : Nat) (xs : List α) : Option Nat :=
    match xs with
    | [] => none
    | x :: xt =>
        if x = a then
          some i
        else
          go (i + 1) xt
  go 0 xs

private structure Indexing (G : ChannelGraph V) where
  edges : List (Sym2 V)
  verts : List V

private def Indexing.ofGraph (G : ChannelGraph V) : Indexing (V := V) G :=
  { edges := G.edges.sort (sym2Le (V := V))
    verts := (Finset.univ : Finset V).sort (· ≤ ·) }

private def Indexing.edgeCount (idx : Indexing (V := V) G) : Nat := idx.edges.length
private def Indexing.vertCount (idx : Indexing (V := V) G) : Nat := idx.verts.length
private def Indexing.edgeBase (_idx : Indexing (V := V) G) : Nat := 1
private def Indexing.vertBase (idx : Indexing (V := V) G) : Nat := idx.edgeBase + idx.edgeCount
private def Indexing.sinkIdx (idx : Indexing (V := V) G) : Nat := idx.vertBase + idx.vertCount
private def Indexing.nodeCount (idx : Indexing (V := V) G) : Nat := idx.sinkIdx + 1
private def Indexing.edgeNodeIdx (idx : Indexing (V := V) G) (ei : Nat) : Nat := idx.edgeBase + ei
private def Indexing.vertNodeIdx (idx : Indexing (V := V) G) (vi : Nat) : Nat := idx.vertBase + vi

private def mkCapMatrix (G : ChannelGraph V) (w : V → Cap) : Indexing (V := V) G × Matrix :=
  let idx := Indexing.ofGraph (V := V) G
  let n := idx.nodeCount
  let src : Nat := 0
  let sink := idx.sinkIdx
  let cap0 : Matrix := Array.replicate n (Array.replicate n 0)

  let setCap (cap : Matrix) (i j : Nat) (x : Cap) : Matrix :=
    cap.set! i ((cap[i]!).set! j x)

  let rec fillEndpoints (eNode : Nat) (c : Cap) (vs : List V) (cap : Matrix) : Matrix :=
    match vs with
    | [] => cap
    | v :: vs =>
        match findIdxList? idx.verts v with
        | none => fillEndpoints eNode c vs cap
        | some vi =>
            let vNode := idx.vertNodeIdx vi
            fillEndpoints eNode c vs (setCap cap eNode vNode c)

  let rec fillEdges (ei : Nat) (es : List (Sym2 V)) (cap : Matrix) : Matrix :=
    match es with
    | [] => cap
    | e :: es =>
        let eNode := idx.edgeNodeIdx ei
        let c := G.cap e
        let cap := setCap cap src eNode c
        let endpoints : List V := (Sym2.toFinset e).sort (· ≤ ·)
        let cap := fillEndpoints eNode c endpoints cap
        fillEdges (ei + 1) es cap

  let rec fillVerts (vi : Nat) (vs : List V) (cap : Matrix) : Matrix :=
    match vs with
    | [] => cap
    | v :: vs =>
        let vNode := idx.vertNodeIdx vi
        let cap := setCap cap vNode sink (w v)
        fillVerts (vi + 1) vs cap

  let cap1 := fillEdges 0 idx.edges cap0
  let cap2 := fillVerts 0 idx.verts cap1
  (idx, cap2)

private def bfsParents (res : Matrix) (src sink : Nat) : Array (Option Nat) :=
  Id.run do
    let n := res.size
    let mut parent : Array (Option Nat) := Array.replicate n none
    parent := parent.set! src (some src)
    let mut q : Array Nat := #[src]
    let mut head : Nat := 0
    while head < q.size do
      let u := q[head]!
      head := head + 1
      if u = sink then
        break
      let row := res[u]!
      for v in [0:n] do
        if parent[v]! = none && row[v]! > 0 then
          parent := parent.set! v (some u)
          q := q.push v
    return parent

private def reachableFromSource (res : Matrix) (src : Nat) : Array Bool :=
  Id.run do
    let n := res.size
    let mut seen : Array Bool := Array.replicate n false
    seen := seen.set! src true
    let mut q : Array Nat := #[src]
    let mut head : Nat := 0
    while head < q.size do
      let u := q[head]!
      head := head + 1
      let row := res[u]!
      for v in [0:n] do
        if (!seen[v]!) && row[v]! > 0 then
          seen := seen.set! v true
          q := q.push v
    return seen

private def bottleneck (res : Matrix) (parent : Array (Option Nat)) (src sink : Nat) (init : Nat) : Nat :=
  Id.run do
    let mut v := sink
    let mut b := init
    while v != src do
      match parent[v]! with
      | none => return 0
      | some u =>
          b := Nat.min b (res[u]![v]!)
          v := u
    return b

private def augment (res : Matrix) (parent : Array (Option Nat)) (src sink delta : Nat) : Matrix :=
  Id.run do
    let mut res := res
    let mut v := sink
    while v != src do
      match parent[v]! with
      | none => break
      | some u =>
          let fwd := res[u]![v]!
          let rev := res[v]![u]!
          res := res.set! u ((res[u]!).set! v (fwd - delta))
          res := res.set! v ((res[v]!).set! u (rev + delta))
          v := u
    return res

private def maxFlow (cap : Matrix) (src sink : Nat) (deltaBound : Nat) : Matrix × Cap :=
  Id.run do
    let n := cap.size
    let mut res := cap
    let mut flow : Cap := 0
    -- Edmonds–Karp terminates in O(VE); we cap iterations by V^3 (since E ≤ V^2).
    let mut fuel : Nat := n * n * n
    while fuel > 0 do
      let parent := bfsParents res src sink
      match parent[sink]! with
      | none => break
      | some _ =>
          let d := bottleneck res parent src sink deltaBound
          if d = 0 then
            break
          res := augment res parent src sink d
          flow := flow + d
      fuel := fuel - 1
    return (res, flow)

private def verifyFeasible (G : ChannelGraph V) (w : V → Cap) (idx : Indexing (V := V) G) (cap res : Matrix) : Bool :=
  let src : Nat := 0
  let sink := idx.sinkIdx
  let flow (i j : Nat) : Cap := cap[i]![j]! - res[i]![j]!

  let rec outSum (eNode : Nat) (vs : List V) : Option Cap :=
    match vs with
    | [] => some 0
    | v :: vs =>
        match findIdxList? idx.verts v, outSum eNode vs with
        | some vi, some s => some (s + flow eNode (idx.vertNodeIdx vi))
        | _, _ => none

  let rec checkEdges (ei : Nat) (es : List (Sym2 V)) : Bool :=
    match es with
    | [] => true
    | e :: es =>
        let c := G.cap e
        let eNode := idx.edgeNodeIdx ei
        if flow src eNode != c then
          false
        else
          let endpoints : List V := (Sym2.toFinset e).sort (· ≤ ·)
          match outSum eNode endpoints with
          | none => false
          | some s =>
              if s != c then
                false
              else
                checkEdges (ei + 1) es

  let rec inSum (ei : Nat) (es : List (Sym2 V)) (vNode : Nat) : Cap :=
    match es with
    | [] => 0
    | _e :: es =>
        let eNode := idx.edgeNodeIdx ei
        (flow eNode vNode) + inSum (ei + 1) es vNode

  let rec checkVerts (vi : Nat) (vs : List V) : Bool :=
    match vs with
    | [] => true
    | v :: vs =>
        let vNode := idx.vertNodeIdx vi
        if inSum 0 idx.edges vNode != w v then
          false
        else if flow vNode sink != w v then
          false
        else
          checkVerts (vi + 1) vs

  checkEdges 0 idx.edges && checkVerts 0 idx.verts

/-- Polynomial-time feasibility checker based on max-flow. -/
def wgFlowBool (G : ChannelGraph V) (w : V → Cap) : Bool :=
  let capTot := totalCap (V := V) G
  if totalWealth (V := V) w != capTot then
    false
  else
    let (idx, cap) := mkCapMatrix (V := V) G w
    let (res, _flow) := maxFlow cap 0 idx.sinkIdx capTot
    verifyFeasible (V := V) G w idx cap res

/-- Attempt to extract a violating cut witness from the residual graph after max-flow.

When `wgFlowBool` fails due to an upper-bound cut violation, the returned `S` should satisfy
`Cuts.cutViolation G w S`. This is intended as a polynomial-time replacement for the exponential
`Cuts.Algorithmic.violatingCuts` search. -/
def violatingCutFlow? (G : ChannelGraph V) (w : V → Cap) : Option (Finset V) :=
  let capTot := totalCap (V := V) G
  let (idx, cap) := mkCapMatrix (V := V) G w
  let (res, _flow) := maxFlow cap 0 idx.sinkIdx capTot
  let seen := reachableFromSource res 0
  let rec build (vi : Nat) (vs : List V) (acc : Finset V) : Finset V :=
    match vs with
    | [] => acc
    | v :: vs =>
        let vNode := idx.vertNodeIdx vi
        let acc := if !seen[vNode]! then insert v acc else acc
        build (vi + 1) vs acc
  let S : Finset V := build 0 idx.verts ∅
  if decide (Cuts.cutViolation (V := V) G w S) then
    some S
  else
    none

end AlgorithmicFlow

end PaymentChannels
end NucleusDB
