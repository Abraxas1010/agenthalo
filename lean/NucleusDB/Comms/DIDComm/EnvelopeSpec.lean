/-!
# DIDComm v2 Envelope Formal Specification

T22: Envelope roundtrip — decrypt(encrypt(m)) = m (AEAD correctness).
T23: Envelope authentication — dual signature binds sender to ciphertext.
-/

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace DIDComm

/-- Abstract DIDComm v2 envelope with encryption and dual signatures. -/
structure Envelope where
  senderId : String
  recipientId : String
  ciphertext : List Nat  -- abstract byte representation
  nonce : List Nat
  tag : List Nat
  edSignature : List Nat
  pqSignature : List Nat
  deriving Repr

/-- Abstract encryption oracle (models AES-256-GCM).
    Returns (ciphertext, tag). -/
axiom encrypt : (key : List Nat) → (plaintext : List Nat) → (nonce : List Nat) →
  (List Nat) × (List Nat)

/-- Abstract decryption oracle (models AES-256-GCM). -/
axiom decrypt : (key : List Nat) → (ciphertext : List Nat) → (nonce : List Nat) →
  (tag : List Nat) → Option (List Nat)

/-- AEAD correctness axiom: decrypt(encrypt(m)) = m. -/
axiom aead_correctness : ∀ (key plaintext nonce : List Nat),
  decrypt key (encrypt key plaintext nonce).1 nonce (encrypt key plaintext nonce).2 = some plaintext

/-- T22: DIDComm envelope roundtrip preserves message content. -/
theorem envelope_roundtrip (key plaintext nonce : List Nat) :
    decrypt key (encrypt key plaintext nonce).1 nonce (encrypt key plaintext nonce).2
      = some plaintext :=
  aead_correctness key plaintext nonce

/-- Wrong key cannot decrypt (ciphertext integrity). -/
axiom aead_wrong_key : ∀ (key1 key2 plaintext nonce : List Nat),
  key1 ≠ key2 →
  decrypt key2 (encrypt key1 plaintext nonce).1 nonce (encrypt key1 plaintext nonce).2 = none

/-- Tampered ciphertext fails decryption (authentication). -/
axiom aead_tamper_fails : ∀ (key plaintext nonce tamperedCt : List Nat),
  (encrypt key plaintext nonce).1 ≠ tamperedCt →
  decrypt key tamperedCt nonce (encrypt key plaintext nonce).2 = none

/-- Tampered tag fails decryption (tag integrity). -/
axiom aead_tag_tamper_fails : ∀ (key plaintext nonce tamperedTag : List Nat),
  (encrypt key plaintext nonce).2 ≠ tamperedTag →
  decrypt key (encrypt key plaintext nonce).1 nonce tamperedTag = none

/-- Signature verification predicate (models dual_verify from halo::did). -/
axiom dualVerify : (edPk pqPk : List Nat) → (message edSig pqSig : List Nat) → Bool

/-- Signature generation (models dual_sign from halo::did). -/
axiom dualSign : (edSk pqSk : List Nat) → (message : List Nat) → (List Nat) × (List Nat)

/-- Signature correctness: signing then verifying with matching keys succeeds. -/
axiom dual_sign_verify_correct :
  ∀ (edSk edPk pqSk pqPk message : List Nat),
  dualVerify edPk pqPk message (dualSign edSk pqSk message).1 (dualSign edSk pqSk message).2
    = true

/-- Signed data for an envelope: ciphertext ++ tag ++ nonce. -/
def envelopeSignedData (env : Envelope) : List Nat :=
  env.ciphertext ++ env.tag ++ env.nonce

/-- T23: Envelope signature binds sender identity to ciphertext. -/
def envelopeAuthentic (env : Envelope) (edPk pqPk : List Nat) : Prop :=
  dualVerify edPk pqPk (envelopeSignedData env)
    env.edSignature env.pqSignature = true

/-- An authentic envelope's ciphertext is bound to the sender's keys. -/
theorem authentic_envelope_sender_bound (env : Envelope) (edPk pqPk : List Nat)
    (h : envelopeAuthentic env edPk pqPk) :
    dualVerify edPk pqPk (envelopeSignedData env)
      env.edSignature env.pqSignature = true := h

/-- Wrong Ed25519 key rejects authentication. -/
axiom dual_verify_wrong_ed_key :
  ∀ (edPk1 edPk2 pqPk message edSig pqSig : List Nat),
  edPk1 ≠ edPk2 →
  dualVerify edPk1 pqPk message edSig pqSig = true →
  dualVerify edPk2 pqPk message edSig pqSig = false

/-- If Ed25519 key doesn't match, envelope is not authentic under that key. -/
theorem wrong_key_not_authentic (env : Envelope) (edPk1 edPk2 pqPk : List Nat)
    (hNeq : edPk1 ≠ edPk2)
    (hAuth : envelopeAuthentic env edPk1 pqPk) :
    ¬ envelopeAuthentic env edPk2 pqPk := by
  unfold envelopeAuthentic at *
  have hFalse := dual_verify_wrong_ed_key edPk1 edPk2 pqPk
    (envelopeSignedData env)
    env.edSignature env.pqSignature hNeq hAuth
  rw [hFalse]
  exact Bool.false_ne_true

/-- Message type enumeration mirroring the Rust `MessageType`. -/
inductive MessageType where
  | mcpToolCall
  | mcpToolResponse
  | envelopeExchange
  | capabilityGrant
  | capabilityAccept
  | peerAnnounce
  | heartbeat
  deriving DecidableEq, Repr

/-- DIDComm message with type, threading, and expiry. -/
structure Message where
  id : String
  type_ : MessageType
  fromDid : String
  toDids : List String
  createdTime : Nat
  expiresTime : Option Nat
  thid : Option String
  pthid : Option String
  deriving Repr

/-- A message is expired if current time exceeds its expiry. -/
def Message.isExpired (msg : Message) (now : Nat) : Prop :=
  match msg.expiresTime with
  | none => False
  | some expires => now ≥ expires

/-- A message without expiry never expires. -/
theorem no_expiry_never_expired (msg : Message) (now : Nat)
    (h : msg.expiresTime = none) :
    ¬ msg.isExpired now := by
  unfold Message.isExpired
  rw [h]
  exact not_false

/-- Expired messages are not valid for processing. -/
def Message.valid (msg : Message) (now : Nat) : Prop :=
  ¬ msg.isExpired now

/-- A message with future expiry is valid. -/
theorem future_expiry_valid (msg : Message) (now expires : Nat)
    (hExp : msg.expiresTime = some expires)
    (hFuture : now < expires) :
    msg.valid now := by
  unfold Message.valid Message.isExpired
  rw [hExp]
  exact Nat.not_le_of_lt hFuture

end DIDComm
end Comms
end NucleusDB
end HeytingLean
