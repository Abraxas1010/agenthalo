import NucleusDB.Crypto.PostQuantum.IncomparableEncoding

/-!
# Crypto.PostQuantum.XMSS

Specification façade for XMSS-style (hash-based) *stateful* signatures.

XMSS signing consumes a one-time epoch / leaf index and advances a monotone counter.
We model this explicitly via:
- a stateful `sign` that returns an updated private key; and
- an `EpochAdvances` property that enforces counter progression.

Conceptual link: this is the same monotone-counter freshness shape as
`HeytingLean.Blockchain.Bridge.MessageModelSeq.deliver` (sequence numbers prevent replay).

Note: the existing stateless signature interface `HeytingLean.Crypto.Signature.Spec.Scheme`
has `sign : Priv → Msg → Sig`. A full XMSS model does not directly fit that interface because
it must return an updated secret key state.
-/

namespace NucleusDB
namespace Crypto
namespace PostQuantum
namespace XMSS

open PostQuantum.IncomparableEncoding

/-- XMSS parameters extend an underlying incomparable-encoding parameter object with XMSS-specific
shape parameters.

Fields:
- `encParam`: parameters for the underlying incomparable encoding
- `height`: Merkle tree height
- `hashBits`: output size (in bits) of the hash used by WOTS+/L-tree
- `w`: Winternitz parameter -/
structure XMSSParams (E : IncomparableEncoding.Scheme) where
  encParam : E.Param
  height   : Nat
  hashBits : Nat
  w        : Nat

/-- Bundled XMSS keypair (public and private key state). -/
structure XMSSKeyPair (Pub Priv : Type) where
  pk : Pub
  sk : Priv

/-- XMSS signatures are tagged with the epoch / leaf index they were produced at. -/
structure XMSSSignature (Epoch Sig : Type) where
  epoch : Epoch
  sig   : Sig

/-- Abstract XMSS-style scheme interface.

This is intentionally a *stateful* signature interface: `sign` may fail (e.g. epoch space
exhausted) and returns an updated private key to model epoch consumption. -/
structure XMSSScheme where
  Enc : IncomparableEncoding.Scheme
  Pub : Type
  Priv : Type
  Sig : Type
  /-- Key generation. -/
  keyGen : XMSSParams Enc → XMSSKeyPair Pub Priv
  /-- Extract the current epoch from a private key state. -/
  epochOf : Priv → Enc.Epoch
  /-- Deterministic epoch advancement function (monotone counter). -/
  nextEpoch : Enc.Epoch → Enc.Epoch
  /-- Stateful signing: returns an updated private key state plus an epoch-tagged signature. -/
  sign : XMSSParams Enc → Priv → Enc.Msg → Option (Priv × XMSSSignature Enc.Epoch Sig)
  /-- Verification predicate. -/
  verify : XMSSParams Enc → Pub → Enc.Msg → XMSSSignature Enc.Epoch Sig → Prop
  /-- Relation between public and private keys. -/
  keyRel : Pub → Priv → Prop

namespace XMSSScheme

/-- Correctness: whenever `sign` succeeds using a private key related to some public key, the
produced signature verifies under that public key. -/
def Correct (X : XMSSScheme) : Prop :=
  ∀ (pp : XMSSParams X.Enc) (pk : X.Pub) (sk sk' : X.Priv) (m : X.Enc.Msg)
      (σ : XMSSSignature X.Enc.Epoch X.Sig),
    X.keyRel pk sk →
    X.sign pp sk m = some (sk', σ) →
    X.verify pp pk m σ

/-- Epoch-advancement property: successful signing consumes the current private-key epoch and
advances the private key's epoch via `nextEpoch`. -/
def EpochAdvances (X : XMSSScheme) : Prop :=
  ∀ (pp : XMSSParams X.Enc) (sk sk' : X.Priv) (m : X.Enc.Msg)
      (σ : XMSSSignature X.Enc.Epoch X.Sig),
    X.sign pp sk m = some (sk', σ) →
      σ.epoch = X.epochOf sk ∧ X.epochOf sk' = X.nextEpoch (X.epochOf sk)

end XMSSScheme

/-! ## Example

A minimal “toy XMSS” instance that demonstrates how the interface is intended to be used.
It models the epoch as a `Nat` counter, increments it on every successful signature, and
verifies all signatures. -/

namespace Example

def enc : IncomparableEncoding.Scheme :=
  { Msg := String
  , Epoch := Nat
  , Rand := Unit
  , Code := Nat × String
  , Param := Unit
  , encode := fun _ e _ m => some (e, m)
  , decode := fun _ e c => if c.1 = e then some c.2 else none }

def scheme : XMSSScheme :=
  { Enc := enc
  , Pub := Unit
  , Priv := Nat
  , Sig := Unit
  , keyGen := fun _ => ⟨(), 0⟩
  , epochOf := fun n => n
  , nextEpoch := Nat.succ
  , sign := fun _ sk _ => some (sk + 1, ⟨sk, ()⟩)
  , verify := fun _ _ _ _ => True
  , keyRel := fun _ _ => True }

theorem scheme_correct : XMSSScheme.Correct scheme := by
  intro pp pk sk sk' m σ _ _
  simp [scheme]

theorem scheme_epochAdvances : XMSSScheme.EpochAdvances scheme := by
  intro pp sk sk' m σ hSign
  simp [scheme] at hSign
  rcases hSign with ⟨hSk', hσ⟩
  constructor
  · have : sk = σ.epoch := by
      simpa using congrArg XMSSSignature.epoch hσ
    simpa [scheme] using this.symm
  · simp [scheme]
    simpa using hSk'.symm

end Example

end XMSS
end PostQuantum
end Crypto
end NucleusDB
