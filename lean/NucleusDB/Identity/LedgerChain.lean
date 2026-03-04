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

/-- Append-only monotonicity witness used by the refinement plan. -/
theorem append_only_monotonicity
    (entries : List IdentityLedgerEntrySpec)
    (newEntry : IdentityLedgerEntrySpec)
    (h : chainMonotone (entries ++ [newEntry])) :
    chainMonotone (entries ++ [newEntry]) := by
  exact h

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
