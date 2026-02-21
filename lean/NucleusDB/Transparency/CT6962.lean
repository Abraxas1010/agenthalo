namespace HeytingLean
namespace NucleusDB
namespace Transparency
namespace RFC6962

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

end RFC6962
end Transparency
end NucleusDB
end HeytingLean
