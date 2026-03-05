import NucleusDB.Identity.LedgerSpec

namespace HeytingLean
namespace NucleusDB
namespace Identity

def chainLinked : List IdentityLedgerEntrySpec → Prop
  | [] => True
  | [_] => True
  | e1 :: e2 :: rest => e2.prevHash = some e1.entryHash ∧ chainLinked (e2 :: rest)

def chainMonotone : List IdentityLedgerEntrySpec → Prop
  | [] => True
  | [_] => True
  | e1 :: e2 :: rest => e2.seq = e1.seq + 1 ∧ chainMonotone (e2 :: rest)

def chainHashesValid : List IdentityLedgerEntrySpec → Prop
  | [] => True
  | e :: rest => hashMatches e ∧ chainHashesValid rest

def wellFormedIdentityChain (entries : List IdentityLedgerEntrySpec) : Prop :=
  chainLinked entries ∧ chainMonotone entries ∧ chainHashesValid entries

def isBindingForDid (did : String) (e : IdentityLedgerEntrySpec) : Bool :=
  match e.kind, e.didSubject with
  | .agentAddressBound, some d => d == did
  | _, _ => false

def latestBindingForDid (did : String) (entries : List IdentityLedgerEntrySpec) : Option IdentityLedgerEntrySpec :=
  entries.foldl
    (fun acc e => if isBindingForDid did e then some e else acc)
    none

/-- Deprecated: tautological witness kept for backward compatibility.
Use `append_preserves_monotone` instead. -/
theorem append_only_monotonicity
    (entries : List IdentityLedgerEntrySpec)
    (newEntry : IdentityLedgerEntrySpec)
    (h : chainMonotone (entries ++ [newEntry])) :
    chainMonotone (entries ++ [newEntry]) := by
  exact h

/-- The last element of `entries ++ [x]` is always `x`. -/
theorem getLast?_append_singleton
    (entries : List IdentityLedgerEntrySpec)
    (x : IdentityLedgerEntrySpec) :
    (entries ++ [x]).getLast? = some x := by
  induction entries with
  | nil =>
      simp
  | cons head tail ih =>
      cases tail with
      | nil =>
          simp
      | cons next rest =>
          simpa [List.cons_append, List.getLast?_cons_cons] using ih

/-- Appending one correctly-sequenced entry preserves sequence monotonicity. -/
theorem append_preserves_monotone
    (entries : List IdentityLedgerEntrySpec)
    (newEntry : IdentityLedgerEntrySpec)
    (hMono : chainMonotone entries)
    (hLink : ∀ last, entries.getLast? = some last → newEntry.seq = last.seq + 1) :
    chainMonotone (entries ++ [newEntry]) := by
  induction entries with
  | nil =>
      simp [chainMonotone]
  | cons head tail ih =>
      cases tail with
      | nil =>
          have hLast : [head].getLast? = some head := by simp
          have hSeq : newEntry.seq = head.seq + 1 := hLink head hLast
          simp [chainMonotone, hSeq]
      | cons mid tail' =>
          simp [chainMonotone] at hMono ⊢
          refine ⟨hMono.1, ?_⟩
          have hTail :
              ∀ last, (mid :: tail').getLast? = some last →
                newEntry.seq = last.seq + 1 := by
            intro last hLast
            exact hLink last (by simpa using hLast)
          exact ih hMono.2 hTail

/-- Appending one correctly-linked entry preserves hash-link structure. -/
theorem append_preserves_linked
    (entries : List IdentityLedgerEntrySpec)
    (newEntry : IdentityLedgerEntrySpec)
    (hLinked : chainLinked entries)
    (hLink : ∀ last, entries.getLast? = some last →
      newEntry.prevHash = some last.entryHash) :
    chainLinked (entries ++ [newEntry]) := by
  induction entries with
  | nil =>
      simp [chainLinked]
  | cons head tail ih =>
      cases tail with
      | nil =>
          have hLast : [head].getLast? = some head := by simp
          have hPrev : newEntry.prevHash = some head.entryHash := hLink head hLast
          simp [chainLinked, hPrev]
      | cons mid tail' =>
          simp [chainLinked] at hLinked ⊢
          refine ⟨hLinked.1, ?_⟩
          have hTail :
              ∀ last, (mid :: tail').getLast? = some last →
                newEntry.prevHash = some last.entryHash := by
            intro last hLast
            exact hLink last (by simpa using hLast)
          exact ih hLinked.2 hTail

/-- Appending one hash-valid entry preserves per-entry hash validity. -/
theorem append_preserves_hashes
    (entries : List IdentityLedgerEntrySpec)
    (newEntry : IdentityLedgerEntrySpec)
    (hHashes : chainHashesValid entries)
    (hNewHash : hashMatches newEntry) :
    chainHashesValid (entries ++ [newEntry]) := by
  induction entries with
  | nil =>
      simp [chainHashesValid, hNewHash]
  | cons head tail ih =>
      simp [chainHashesValid] at hHashes ⊢
      exact ⟨hHashes.1, ih hHashes.2⟩

/-- Provenance composability for monotone chains under shared boundary witness. -/
theorem chain_transitivity
    (entries : List IdentityLedgerEntrySpec)
    (a b : IdentityLedgerEntrySpec)
    (hMono : chainMonotone (entries ++ [a]))
    (hSeq : b.seq = a.seq + 1) :
    chainMonotone (entries ++ [a, b]) := by
  have hExt :
      chainMonotone ((entries ++ [a]) ++ [b]) := by
    refine append_preserves_monotone (entries ++ [a]) b hMono ?_
    intro last hLast
    have hEqSome :
        (some last : Option IdentityLedgerEntrySpec) = some a := by
      simpa [hLast] using getLast?_append_singleton entries a
    have hLastEq : last = a := Option.some.inj hEqSome
    simpa [hLastEq] using hSeq
  simpa [List.append_assoc] using hExt

/-- Provenance composability: appending a correctly linked, sequenced, and
hash-valid entry preserves full chain well-formedness. -/
theorem chain_composable
    (entries : List IdentityLedgerEntrySpec)
    (a b : IdentityLedgerEntrySpec)
    (ha : wellFormedIdentityChain (entries ++ [a]))
    (hLink : b.prevHash = some a.entryHash)
    (hSeq : b.seq = a.seq + 1)
    (hHash : hashMatches b) :
    wellFormedIdentityChain (entries ++ [a, b]) := by
  rcases ha with ⟨hLinked, hMono, hHashes⟩
  have hLinkedExt : chainLinked ((entries ++ [a]) ++ [b]) := by
    refine append_preserves_linked (entries ++ [a]) b hLinked ?_
    intro last hLast
    have hEqSome :
        (some last : Option IdentityLedgerEntrySpec) = some a := by
      simpa [hLast] using getLast?_append_singleton entries a
    have hLastEq : last = a := Option.some.inj hEqSome
    simpa [hLastEq] using hLink
  have hMonoExt : chainMonotone ((entries ++ [a]) ++ [b]) :=
    append_preserves_monotone (entries ++ [a]) b hMono (by
      intro last hLast
      have hEqSome :
          (some last : Option IdentityLedgerEntrySpec) = some a := by
        simpa [hLast] using getLast?_append_singleton entries a
      have hLastEq : last = a := Option.some.inj hEqSome
      simpa [hLastEq] using hSeq)
  have hHashesExt : chainHashesValid ((entries ++ [a]) ++ [b]) :=
    append_preserves_hashes (entries ++ [a]) b hHashes hHash
  refine ⟨?_, ?_, ?_⟩
  · simpa [List.append_assoc] using hLinkedExt
  · simpa [List.append_assoc] using hMonoExt
  · simpa [List.append_assoc] using hHashesExt

/-- If an entry hash no longer matches computed hash, local validity fails. -/
theorem tampering_detectable
    (e : IdentityLedgerEntrySpec)
    (hTamper : e.entryHash ≠ compute_entry_hash e) :
    ¬ hashMatches e := by
  unfold hashMatches
  exact hTamper

/-- Lookup result is always a bound-address event for the queried DID. -/
theorem lookup_returns_latest
    (did : String)
    (entries : List IdentityLedgerEntrySpec)
    (e : IdentityLedgerEntrySpec)
    (h : latestBindingForDid did entries = some e) :
    isBindingForDid did e = true := by
  unfold latestBindingForDid at h
  let step := fun (acc : Option IdentityLedgerEntrySpec) (cur : IdentityLedgerEntrySpec) =>
    if isBindingForDid did cur then some cur else acc
  have hInv :
      ∀ ys acc out,
        (∀ q, acc = some q → isBindingForDid did q = true) →
        List.foldl step acc ys = some out →
        isBindingForDid did out = true := by
    intro ys
    induction ys with
    | nil =>
        intro acc out hAcc hFold
        simpa using hAcc out hFold
    | cons y ys ih =>
        intro acc out hAcc hFold
        simp [step] at hFold
        by_cases hy : isBindingForDid did y = true
        · simp [hy] at hFold
          refine ih (some y) out ?hAcc hFold
          intro q hq
          injection hq with hq'
          subst hq'
          exact hy
        · exact ih acc out hAcc (by simpa [hy] using hFold)
  exact hInv entries none e (by intro q hq; cases hq) (by simpa [step] using h)

end Identity
end NucleusDB
end HeytingLean
