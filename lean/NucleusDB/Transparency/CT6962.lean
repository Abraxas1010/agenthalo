import Mathlib.Data.List.Induction

namespace HeytingLean
namespace NucleusDB
namespace Transparency
namespace RFC6962

open List

/-- Abstract RFC6962-style Merkle hashing interface. -/
structure MerkleHashSpec where
  Hash : Type
  emptyHash : Hash
  leafHash : String → Hash
  nodeHash : Hash → Hash → Hash

/-- Inclusion proof payload (RFC6962-style index/path witness). -/
structure InclusionProof (H : Type) where
  leafIndex : Nat
  treeSize : Nat
  leafHash : H
  path : List H
  expectedRoot : H

/-- Consistency proof payload linking old/new tree heads. -/
structure ConsistencyProof (H : Type) where
  oldSize : Nat
  newSize : Nat
  oldRoot : H
  newRoot : H
  path : List H

/-- Replay inclusion path under an abstract hash spec.
This is a simplified executable replay kernel used by the proof surface. -/
def replayInclusionPath (S : MerkleHashSpec) (h : S.Hash) (path : List S.Hash) : S.Hash :=
  path.foldl S.nodeHash h

/-- Model-level inclusion verifier predicate.
A proof is accepted only when index bounds hold and replayed root matches. -/
def verifyInclusionProof (S : MerkleHashSpec) (π : InclusionProof S.Hash) : Prop :=
  π.leafIndex < π.treeSize ∧ replayInclusionPath S π.leafHash π.path = π.expectedRoot

/-- Soundness: accepted inclusion proofs establish the claimed root relation. -/
theorem verifyInclusionProof_sound
    (S : MerkleHashSpec)
    (π : InclusionProof S.Hash)
    (h : verifyInclusionProof S π) :
    replayInclusionPath S π.leafHash π.path = π.expectedRoot :=
  h.2

/-- Replay kernel for consistency proofs.
Returns the roots reconstructed from the proof witness. -/
def replayConsistencyPath (S : MerkleHashSpec)
    (seed : S.Hash)
    (path : List S.Hash) : S.Hash × S.Hash :=
  path.foldl (fun (acc : S.Hash × S.Hash) x => (S.nodeHash x acc.1, S.nodeHash x acc.2))
    (seed, seed)

/-- Model-level consistency verifier predicate.
The checker is fail-closed: any size or root mismatch rejects. -/
def verifyConsistencyProof (S : MerkleHashSpec) (π : ConsistencyProof S.Hash) : Prop :=
  π.oldSize ≤ π.newSize ∧
    if _hEq : π.oldSize = π.newSize then
      π.oldRoot = π.newRoot ∧ π.path = []
    else
      match π.path with
      | [] => False
      | seed :: tail =>
          let r := replayConsistencyPath S seed tail
          r.1 = π.oldRoot ∧ r.2 = π.newRoot

/-- Consistency acceptance implies monotone tree-size extension. -/
theorem verifyConsistencyProof_implies_size_extension
    (S : MerkleHashSpec)
    (π : ConsistencyProof S.Hash)
    (h : verifyConsistencyProof S π) :
    π.oldSize ≤ π.newSize :=
  h.1

/-- Equal-size consistency acceptance implies identical roots and empty witness path. -/
theorem verifyConsistencyProof_sound
    (S : MerkleHashSpec)
    (π : ConsistencyProof S.Hash)
    (h : verifyConsistencyProof S π) :
    π.oldSize = π.newSize → π.oldRoot = π.newRoot ∧ π.path = [] := by
  intro hEq
  simpa [verifyConsistencyProof, hEq] using h.2

/-- Strict-size-extension acceptance implies replayed roots match the claimed heads. -/
theorem verifyConsistencyProof_replay_matches_roots
    (S : MerkleHashSpec)
    (π : ConsistencyProof S.Hash)
    (h : verifyConsistencyProof S π)
    (hLt : π.oldSize < π.newSize) :
    match π.path with
    | [] => False
    | seed :: tail =>
        let r := replayConsistencyPath S seed tail
        r.1 = π.oldRoot ∧ r.2 = π.newRoot := by
  have hNe : π.oldSize ≠ π.newSize := Nat.ne_of_lt hLt
  simpa [verifyConsistencyProof, hNe] using h.2

structure CollisionResistant (S : MerkleHashSpec) : Prop where
  emptyHash_ne_nodeHash : ∀ x y, S.emptyHash ≠ S.nodeHash x y
  nodeHash_injective :
    ∀ {a b c d : S.Hash}, S.nodeHash a b = S.nodeHash c d → a = c ∧ b = d

def leafChainRoot (S : MerkleHashSpec) (leaves : List S.Hash) : S.Hash :=
  leaves.foldl S.nodeHash S.emptyHash

def replayAppendPath (S : MerkleHashSpec) (oldRoot : S.Hash) (suffix : List S.Hash) : S.Hash :=
  suffix.foldl S.nodeHash oldRoot

def verifyAppendOnlyConsistencyProof
    (S : MerkleHashSpec) (π : ConsistencyProof S.Hash) : Prop :=
  π.oldSize ≤ π.newSize ∧
    if _hEq : π.oldSize = π.newSize then
      π.oldRoot = π.newRoot ∧ π.path = []
    else
      π.newRoot = replayAppendPath S π.oldRoot π.path

theorem leafChainRoot_append
    (S : MerkleHashSpec) (xs ys : List S.Hash) :
    leafChainRoot S (xs ++ ys) = replayAppendPath S (leafChainRoot S xs) ys := by
  simp [leafChainRoot, replayAppendPath, List.foldl_append]

theorem leafChainRoot_snoc
    (S : MerkleHashSpec) (xs : List S.Hash) (x : S.Hash) :
    leafChainRoot S (xs ++ [x]) = S.nodeHash (leafChainRoot S xs) x := by
  simpa [replayAppendPath] using leafChainRoot_append S xs [x]

theorem leafChainRoot_ne_empty_of_ne_nil
    (S : MerkleHashSpec) (hCR : CollisionResistant S)
    {xs : List S.Hash} (hxs : xs ≠ []) :
    leafChainRoot S xs ≠ S.emptyHash := by
  induction xs using List.reverseRecOn with
  | nil =>
      cases hxs rfl
  | append_singleton init last _ =>
      rw [leafChainRoot_snoc]
      exact fun hEq => hCR.emptyHash_ne_nodeHash (leafChainRoot S init) last hEq.symm

theorem leafChainRoot_injective
    (S : MerkleHashSpec) (hCR : CollisionResistant S) :
    Function.Injective (leafChainRoot S) := by
  intro xs ys hEq
  induction xs using List.reverseRecOn generalizing ys with
  | nil =>
      cases ys using List.reverseRecOn with
      | nil =>
          rfl
      | append_singleton init last _ =>
          exfalso
          exact leafChainRoot_ne_empty_of_ne_nil S hCR (by simp) hEq.symm
  | append_singleton init last ih =>
      cases ys using List.reverseRecOn with
      | nil =>
          exfalso
          exact leafChainRoot_ne_empty_of_ne_nil S hCR (by simp) hEq
      | append_singleton init' last' _ =>
          rw [leafChainRoot_snoc, leafChainRoot_snoc] at hEq
          rcases hCR.nodeHash_injective hEq with ⟨hRoots, hLast⟩
          have hInit : init = init' := ih hRoots
          subst hInit
          subst hLast
          rfl

theorem verifyAppendOnlyConsistencyProof_implies_size_extension
    (S : MerkleHashSpec)
    (π : ConsistencyProof S.Hash)
    (h : verifyAppendOnlyConsistencyProof S π) :
    π.oldSize ≤ π.newSize :=
  h.1

theorem verifyAppendOnlyConsistencyProof_sound
    (S : MerkleHashSpec)
    (π : ConsistencyProof S.Hash)
    (h : verifyAppendOnlyConsistencyProof S π) :
    π.oldSize = π.newSize → π.oldRoot = π.newRoot ∧ π.path = [] := by
  intro hEq
  simpa [verifyAppendOnlyConsistencyProof, hEq] using h.2

theorem verifyAppendOnlyConsistencyProof_replay_matches_new_root
    (S : MerkleHashSpec)
    (π : ConsistencyProof S.Hash)
    (h : verifyAppendOnlyConsistencyProof S π)
    (hLt : π.oldSize < π.newSize) :
    π.newRoot = replayAppendPath S π.oldRoot π.path := by
  have hNe : π.oldSize ≠ π.newSize := Nat.ne_of_lt hLt
  simpa [verifyAppendOnlyConsistencyProof, hNe] using h.2

theorem consistency_implies_prefix
    (S : MerkleHashSpec)
    (hCR : CollisionResistant S)
    (π : ConsistencyProof S.Hash)
    (oldLeaves newLeaves : List S.Hash)
    (h : verifyAppendOnlyConsistencyProof S π)
    (hOldRoot : leafChainRoot S oldLeaves = π.oldRoot)
    (hNewRoot : leafChainRoot S newLeaves = π.newRoot) :
    List.IsPrefix oldLeaves newLeaves := by
  by_cases hEq : π.oldSize = π.newSize
  · have hRoots : π.oldRoot = π.newRoot ∧ π.path = [] :=
      verifyAppendOnlyConsistencyProof_sound S π h hEq
    have hSameRoot : leafChainRoot S oldLeaves = leafChainRoot S newLeaves := by
      rw [hOldRoot, hRoots.1, ← hNewRoot]
    have hSameLeaves : oldLeaves = newLeaves :=
      leafChainRoot_injective S hCR hSameRoot
    subst hSameLeaves
    exact ⟨[], by simp⟩
  · have hReplay :
      π.newRoot = replayAppendPath S π.oldRoot π.path :=
        verifyAppendOnlyConsistencyProof_replay_matches_new_root
          S π h (Nat.lt_of_le_of_ne h.1 hEq)
    have hRoots :
        leafChainRoot S newLeaves = leafChainRoot S (oldLeaves ++ π.path) := by
      calc
        leafChainRoot S newLeaves = π.newRoot := hNewRoot
        _ = replayAppendPath S π.oldRoot π.path := hReplay
        _ = replayAppendPath S (leafChainRoot S oldLeaves) π.path := by
              simpa [hOldRoot]
        _ = leafChainRoot S (oldLeaves ++ π.path) := by
              symm
              exact leafChainRoot_append S oldLeaves π.path
    have hLeaves :
        newLeaves = oldLeaves ++ π.path :=
      leafChainRoot_injective S hCR hRoots
    exact ⟨π.path, hLeaves.symm⟩

end RFC6962
end Transparency
end NucleusDB
end HeytingLean
