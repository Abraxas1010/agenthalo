import Mathlib.Data.List.Basic
import NucleusDB.PaymentChannels.MultiChain.Graph

namespace NucleusDB
namespace PaymentChannels
namespace MultiChain

/-!
# NucleusDB.PaymentChannels.MultiChain.CrossChainReplay

Cross-chain replay model where source/destination chain IDs are included in the hash domain.
-/

abbrev Seq := Nat
abbrev Payload := String

structure CrossChainHash where
  src : ChainId
  dst : ChainId
  seq : Seq
  payload : Payload
  deriving DecidableEq, Repr

structure CrossChainMessage where
  srcChain : ChainId
  dstChain : ChainId
  seq : Seq
  payload : Payload
  hash : CrossChainHash
  deriving DecidableEq, Repr

def domainSeparatedHash
    (src dst : ChainId) (seq : Seq) (payload : Payload) : CrossChainHash :=
  { src := src, dst := dst, seq := seq, payload := payload }

def hashWellFormed (m : CrossChainMessage) : Prop :=
  m.hash = domainSeparatedHash m.srcChain m.dstChain m.seq m.payload

instance (m : CrossChainMessage) : Decidable (hashWellFormed m) := by
  unfold hashWellFormed
  infer_instance

abbrev MsgKey := ChainId × Seq

def CrossChainMessage.key (m : CrossChainMessage) : MsgKey :=
  (m.srcChain, m.seq)

structure CrossChainState where
  sent : List CrossChainMessage
  delivered : List CrossChainMessage
  maxSeq : ChainId → Seq


def updateMaxSeq (f : ChainId → Seq) (src : ChainId) (seq : Seq) : ChainId → Seq :=
  fun c => if c = src then seq else f c

/-- Replay and integrity invariants for delivered messages. -/
def Invariants (st : CrossChainState) : Prop :=
  (st.delivered.map CrossChainMessage.key).Nodup ∧
    (∀ m, m ∈ st.delivered → m ∈ st.sent ∧ hashWellFormed m) ∧
    (∀ m, m ∈ st.delivered → m.seq ≤ st.maxSeq m.srcChain)

/-- Deliver only if message was sent, hash is well-formed, and seq is strictly fresh. -/
def crosschain_deliver (st : CrossChainState) (m : CrossChainMessage) : CrossChainState :=
  if _ : m ∈ st.sent then
    if _ : hashWellFormed m then
      if _ : st.maxSeq m.srcChain < m.seq then
        { st with
          delivered := m :: st.delivered
          maxSeq := updateMaxSeq st.maxSeq m.srcChain m.seq }
      else st
    else st
  else st

private lemma not_mem_keys_of_lt_maxSeq
    {st : CrossChainState} {m : CrossChainMessage}
    (hInv : Invariants st)
    (hFresh : st.maxSeq m.srcChain < m.seq) :
    CrossChainMessage.key m ∉ st.delivered.map CrossChainMessage.key := by
  intro hMem
  rcases List.exists_of_mem_map hMem with ⟨m', hm', hkey⟩
  have hSeqLe : m'.seq ≤ st.maxSeq m'.srcChain := (hInv.right.right) m' hm'
  have hSrcEq : m'.srcChain = m.srcChain := by
    have := congrArg Prod.fst hkey
    simpa [CrossChainMessage.key] using this
  have hSeqEq : m'.seq = m.seq := by
    have := congrArg Prod.snd hkey
    simpa [CrossChainMessage.key] using this
  have : m.seq ≤ st.maxSeq m.srcChain := by
    simpa [hSrcEq, hSeqEq] using hSeqLe
  exact (not_le_of_gt hFresh) this

/-- Cross-chain delivery preserves replay/integrity invariants. -/
theorem crosschain_deliver_preserves_invariants
    {st : CrossChainState} {m : CrossChainMessage}
    (hInv : Invariants st) :
    Invariants (crosschain_deliver st m) := by
  unfold crosschain_deliver
  by_cases hSent : m ∈ st.sent
  · by_cases hHash : hashWellFormed m
    · by_cases hFresh : st.maxSeq m.srcChain < m.seq
      · rcases hInv with ⟨hNodup, hSentHash, hUpper⟩
        have hNotMem : CrossChainMessage.key m ∉ st.delivered.map CrossChainMessage.key :=
          not_mem_keys_of_lt_maxSeq ⟨hNodup, hSentHash, hUpper⟩ hFresh
        have hNodup' : (CrossChainMessage.key m :: st.delivered.map CrossChainMessage.key).Nodup :=
          List.nodup_cons.mpr ⟨hNotMem, hNodup⟩
        have hSentHash' :
            ∀ m', m' ∈ (m :: st.delivered) → m' ∈ st.sent ∧ hashWellFormed m' := by
          intro m' hm'
          simp at hm'
          rcases hm' with rfl | hm'
          · exact ⟨hSent, hHash⟩
          · exact hSentHash m' hm'
        have hUpper' :
            ∀ m', m' ∈ (m :: st.delivered) →
              m'.seq ≤ (updateMaxSeq st.maxSeq m.srcChain m.seq) m'.srcChain := by
          intro m' hm'
          simp at hm'
          rcases hm' with rfl | hm'
          · simp [updateMaxSeq]
          · have hLeOld : m'.seq ≤ st.maxSeq m'.srcChain := hUpper m' hm'
            by_cases hSrc : m'.srcChain = m.srcChain
            · have hLt : m'.seq < m.seq := lt_of_le_of_lt hLeOld (by simpa [hSrc] using hFresh)
              have hLeNew : m'.seq ≤ m.seq := le_of_lt hLt
              simpa [updateMaxSeq, hSrc] using hLeNew
            · simpa [updateMaxSeq, hSrc] using hLeOld
        refine ⟨?_, ?_, ?_⟩
        · simpa [hSent, hHash, hFresh] using hNodup'
        · intro m' hm'
          simp [hSent, hHash, hFresh] at hm'
          have hmMem : m' ∈ m :: st.delivered := by
            simpa using hm'
          simpa [hSent, hHash, hFresh] using (hSentHash' m' hmMem)
        · intro m' hm'
          simp [hSent, hHash, hFresh] at hm'
          have hmMem : m' ∈ m :: st.delivered := by
            simpa using hm'
          simpa [hSent, hHash, hFresh] using (hUpper' m' hmMem)
      · simpa [hSent, hHash, hFresh] using hInv
    · simpa [hSent, hHash] using hInv
  · simpa [hSent] using hInv

/-- Cross-chain reinterpretation is blocked because source/destination are hash-domain inputs. -/
theorem no_crosschain_replay
    (m : CrossChainMessage)
    (hHash : hashWellFormed m)
    (hSrcDst : m.srcChain ≠ m.dstChain) :
    domainSeparatedHash m.dstChain m.srcChain m.seq m.payload ≠ m.hash := by
  intro hSwap
  have hEq :
      domainSeparatedHash m.dstChain m.srcChain m.seq m.payload =
      domainSeparatedHash m.srcChain m.dstChain m.seq m.payload := by
    exact hSwap.trans hHash
  have hChains : m.dstChain = m.srcChain := by
    simpa [domainSeparatedHash] using congrArg CrossChainHash.src hEq
  exact hSrcDst hChains.symm

end MultiChain
end PaymentChannels
end NucleusDB
