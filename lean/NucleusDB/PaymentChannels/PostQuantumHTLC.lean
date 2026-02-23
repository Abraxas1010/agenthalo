import NucleusDB.Bridge.MessageModelSeq
import NucleusDB.Crypto.PostQuantum.XMSS

/-!
# Blockchain.PaymentChannels.PostQuantumHTLC

Spec-level HTLC definitions intended for post-quantum payment-channel settlement models.

Key integration points:
- Hash-locks are parameterized over an abstract hash `Preimage → Digest` (the “post-quantum”
  aspect is the choice of digest/hash backend, not an extra postulate).
- Settlement updates should carry monotone sequence numbers to prevent replay; this mirrors
  `HeytingLean.Blockchain.Bridge.MessageModelSeq`'s freshness rule.
- XMSS-style stateful signatures carry an epoch counter; `Crypto.PostQuantum.XMSS.XMSSScheme.EpochAdvances`
  matches the same monotone-counter shape and is meant to justify no-replay properties for signed updates.
-/

namespace NucleusDB
namespace PaymentChannels
namespace PostQuantumHTLC

/-- Sequence numbers for settlement / replay prevention. -/
abbrev Seq := NucleusDB.Bridge.MessageModelSeq.Seq

/-- Generic hash-lock (parameterized over `Digest` / `Preimage`). -/
structure HashLock (Digest Preimage : Type) where
  commitment : Digest
  timeout    : Nat

/-- A payment-channel HTLC with sender/receiver/amount and a hash-lock. -/
structure HTLC (Address Digest Preimage : Type) where
  sender   : Address
  receiver : Address
  amount   : Nat
  lock     : HashLock Digest Preimage

variable {Address Digest Preimage : Type}

/-- The HTLC is claimable if the witness hashes to the commitment. -/
def claimable (hash : Preimage → Digest) (h : HTLC Address Digest Preimage) (w : Preimage) : Prop :=
  hash w = h.lock.commitment

/-- The HTLC is refundable once the timeout has passed. -/
def refundable (h : HTLC Address Digest Preimage) (currentBlock : Nat) : Prop :=
  currentBlock ≥ h.lock.timeout

/-! ## Monotone sequence-number settlement scaffolding -/

/-- Minimal per-author sequence tracking state. -/
structure SeqState (Address : Type) where
  maxSeq : Address → Seq

variable {Payload : Type} [DecidableEq Address]

/-- Generic update with a monotone sequence number. -/
structure Update (Address Payload : Type) where
  author  : Address
  seq     : Seq
  payload : Payload

/-- Freshness predicate: the update sequence number strictly exceeds the stored maximum. -/
def Fresh (st : SeqState Address) (u : Update Address Payload) : Prop :=
  st.maxSeq u.author < u.seq

private def updateMaxSeq (f : Address → Seq) (a : Address) (s : Seq) : Address → Seq :=
  fun a' => if a' = a then s else f a'

/-- Apply an update if it is fresh; otherwise leave the state unchanged. -/
def apply (st : SeqState Address) (u : Update Address Payload) : SeqState Address :=
  if st.maxSeq u.author < u.seq then
    { maxSeq := updateMaxSeq st.maxSeq u.author u.seq }
  else
    st

theorem apply_maxSeq_eq_of_fresh (st : SeqState Address) (u : Update Address Payload)
    (hFresh : Fresh st u) :
    (apply st u).maxSeq u.author = u.seq := by
  unfold Fresh at hFresh
  unfold apply
  simp [hFresh, updateMaxSeq]

/-! ## XMSS hooks -/

namespace Signed

open NucleusDB.Crypto.PostQuantum.XMSS

/-- A signed message bundle suitable for settlement updates. -/
structure Bundle (X : XMSSScheme) where
  params : XMSSParams X.Enc
  pk     : X.Pub
  msg    : X.Enc.Msg
  sig    : XMSSSignature X.Enc.Epoch X.Sig

/-- Verification predicate for a signed bundle. -/
def Verifies (X : XMSSScheme) (b : Bundle X) : Prop :=
  X.verify b.params b.pk b.msg b.sig

/-- Re-export the epoch-advancement property as the intended no-replay assumption for XMSS-style signing. -/
def NoReplayAssumption (X : XMSSScheme) : Prop :=
  XMSSScheme.EpochAdvances X

end Signed

end PostQuantumHTLC
end PaymentChannels
end NucleusDB
