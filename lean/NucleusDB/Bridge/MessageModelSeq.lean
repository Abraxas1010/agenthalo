import Mathlib.Data.List.Basic

/-!
# Blockchain.Bridge.MessageModelSeq

Sequence-number (freshness) extension of `Blockchain.Bridge.MessageModel`.

Motivation / external patterns:
- EIP-778 ENR freshness is governed by a monotone `seq` field (must increase on updates).
- Noise/ChaCha20-Poly1305 encryption requires a monotone nonce/counter (nonce reuse is catastrophic).

This module models the shared “monotone counter prevents replay” idea as a tiny state machine:
messages are delivered iff they were sent *and* have a strictly-fresh `seq` relative to the sender.

This is intended as a reusable building block for:
- bridge-style message passing invariants; and
- payment-channel settlement “state update must have higher sequence number” rules.
-/

namespace NucleusDB
namespace Bridge
namespace MessageModelSeq

/-- Chain identifiers for this minimal model. -/
abbrev ChainId := Nat

/-- Sequence numbers (nonces). -/
abbrev Seq := Nat

/-- Payloads are kept abstract at this level. -/
abbrev Payload := String

/-- A message with a monotone per-source sequence number. -/
structure Message where
  src     : ChainId
  dst     : ChainId
  seq     : Seq
  payload : Payload
  deriving Repr, DecidableEq

/-- Replay key: (source, sequence number). -/
abbrev MsgKey := ChainId × Seq

/-- Extract the replay key from a message. -/
def Message.key (m : Message) : MsgKey := (m.src, m.seq)

/-- Bridge state:
* a set of sent messages;
* a list of delivered messages; and
* a per-source upper bound `maxSeq` tracking the highest delivered `seq`.
-/
structure State where
  sent      : List Message
  delivered : List Message
  maxSeq    : ChainId → Seq

/-- Update the `maxSeq` function at a specific source. -/
def updateMaxSeq (f : ChainId → Seq) (src : ChainId) (seq : Seq) : ChainId → Seq :=
  fun c => if c = src then seq else f c

/-- Invariants:
* delivered replay-keys are duplicate-free;
* every delivered message was previously sent; and
* `maxSeq` upper-bounds all delivered sequence numbers per source.
-/
def Invariants (st : State) : Prop :=
  (st.delivered.map Message.key).Nodup ∧
    (∀ m, m ∈ st.delivered → m ∈ st.sent) ∧
    (∀ m, m ∈ st.delivered → m.seq ≤ st.maxSeq m.src)

/-- Single-step delivery with freshness:
deliver `m` only if:
1) it was sent, and
2) it is strictly fresh: `st.maxSeq m.src < m.seq`.

On successful delivery, `maxSeq m.src` is updated to `m.seq`.
-/
def deliver (st : State) (m : Message) : State :=
  if _ : m ∈ st.sent then
    if _ : st.maxSeq m.src < m.seq then
      { st with
        delivered := m :: st.delivered
        maxSeq := updateMaxSeq st.maxSeq m.src m.seq }
    else
      st
  else
    st

/-- Iterate `deliver` over a list of messages. -/
def runDeliveries (st : State) (ms : List Message) : State :=
  ms.foldl deliver st

section Lemmas

variable {st : State} {m : Message} {ms : List Message}

private lemma not_mem_keys_of_lt_maxSeq
    (hInv : Invariants st)
    (hFresh : st.maxSeq m.src < m.seq) :
    Message.key m ∉ st.delivered.map Message.key := by
  classical
  intro hMem
  rcases List.exists_of_mem_map hMem with ⟨m', hm', hkey⟩
  have hSeqLe : m'.seq ≤ st.maxSeq m'.src :=
    (hInv.right.right) m' hm'
  have hSrcEq : m'.src = m.src := by
    have : m'.key.1 = m.key.1 := congrArg Prod.fst hkey
    simpa [Message.key] using this
  have hSeqEq : m'.seq = m.seq := by
    have : m'.key.2 = m.key.2 := congrArg Prod.snd hkey
    simpa [Message.key] using this
  have : m.seq ≤ st.maxSeq m.src := by
    simpa [hSrcEq, hSeqEq] using hSeqLe
  exact (not_le_of_gt hFresh) this

/-- `deliver` preserves the invariants. -/
theorem deliver_preserves_invariants
    (hInv : Invariants st) :
    Invariants (deliver st m) := by
  classical
  unfold deliver
  by_cases hSent : m ∈ st.sent
  · by_cases hFresh : st.maxSeq m.src < m.seq
    · -- Delivered with freshness: update `delivered` and `maxSeq`.
      have hNotMemKeys : Message.key m ∉ st.delivered.map Message.key :=
        not_mem_keys_of_lt_maxSeq (st := st) (m := m) hInv hFresh
      -- Unpack invariants.
      rcases hInv with ⟨hNodupKeys, hDeliveredSent, hUpper⟩
      -- New keys are `m.key :: oldKeys`.
      have hNodupKeys' : (Message.key m :: st.delivered.map Message.key).Nodup := by
        exact List.nodup_cons.mpr ⟨hNotMemKeys, hNodupKeys⟩
      have hDeliveredSent' : ∀ m', m' ∈ (m :: st.delivered) → m' ∈ st.sent := by
        intro m' hm'
        simp at hm'
        rcases hm' with rfl | hm'
        · exact hSent
        · exact hDeliveredSent m' hm'
      have hUpper' :
          ∀ m', m' ∈ (m :: st.delivered) →
            m'.seq ≤ (updateMaxSeq st.maxSeq m.src m.seq) m'.src := by
        intro m' hm'
        simp at hm'
        rcases hm' with rfl | hm'
        · -- For the newly delivered message, `maxSeq` is set to `m.seq`.
          simp [updateMaxSeq]
        · -- For previously delivered messages, `maxSeq` is unchanged except possibly at `m.src`.
          have hLeOld : m'.seq ≤ st.maxSeq m'.src := hUpper m' hm'
          by_cases hSrc : m'.src = m.src
          · -- Source matches: old `maxSeq` is strictly below the new one (`m.seq`).
            have hLt : m'.seq < m.seq :=
              lt_of_le_of_lt (a := m'.seq) (b := st.maxSeq m'.src) (c := m.seq) hLeOld (by simpa [hSrc] using hFresh)
            have hLeNew : m'.seq ≤ m.seq := le_of_lt hLt
            simpa [updateMaxSeq, hSrc] using hLeNew
          · -- Different source: `maxSeq` unchanged.
            simpa [updateMaxSeq, hSrc] using hLeOld
      refine ⟨?_, ?_, ?_⟩
      · -- `delivered.map key` is duplicate-free.
        simp only [dif_pos hSent, dif_pos hFresh]
        simpa using hNodupKeys'
      · -- Every delivered message was sent.
        intro m' hm'
        simp only [dif_pos hSent, dif_pos hFresh] at hm' ⊢
        exact hDeliveredSent' m' hm'
      · -- Upper-bound property for `maxSeq`.
        intro m' hm'
        simp only [dif_pos hSent, dif_pos hFresh] at hm' ⊢
        exact hUpper' m' hm'
    · -- Not fresh: state unchanged.
      simp only [dif_pos hSent, dif_neg hFresh]
      exact hInv
  · -- Not sent: state unchanged.
    simp only [dif_neg hSent]
    exact hInv

/-- `runDeliveries` preserves invariants. -/
theorem runDeliveries_preserves_invariants
    (st : State) (ms : List Message) :
    Invariants st → Invariants (runDeliveries st ms) := by
  classical
  induction ms generalizing st with
  | nil =>
      intro hInv
      simpa [runDeliveries] using hInv
  | cons m ms ih =>
      intro hInv
      have hStep : Invariants (deliver st m) :=
        deliver_preserves_invariants (st := st) (m := m) hInv
      have hTail : Invariants (runDeliveries (deliver st m) ms) :=
        ih (st := deliver st m) hStep
      simpa [runDeliveries, List.foldl] using hTail

/-- Replay-prevention statement: delivered keys remain duplicate-free across any run. -/
def ReplayPreventionStatement : Prop :=
  ∀ st ms, Invariants st →
    ((runDeliveries st ms).delivered.map Message.key).Nodup

theorem replayPreventionStatement_holds :
    ReplayPreventionStatement := by
  intro st ms hInv
  have hInv' := runDeliveries_preserves_invariants (st := st) (ms := ms) hInv
  exact hInv'.left

end Lemmas

end MessageModelSeq
end Bridge
end NucleusDB
