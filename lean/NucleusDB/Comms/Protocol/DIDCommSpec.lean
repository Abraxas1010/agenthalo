namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

/-- Minimal abstract authcrypt envelope witness used by the formal model. -/
structure AuthcryptEnvelopeSpec where
  senderDid : String
  ed25519SigValid : Bool
  mlDsa65SigValid : Bool
  decrypts : Bool
  expiresTime : Option Nat
  deriving DecidableEq, Repr

/-- Runtime expiry gate: a message is valid if there is no expiry, or `now ≤ expires`. -/
def notExpiredAt (now : Nat) (expiresTime : Option Nat) : Prop :=
  match expiresTime with
  | none => True
  | some expires => now ≤ expires

/-- Acceptance predicate matching the runtime `unpack_with_resolver` gate shape. -/
def acceptsAuthcrypt (env : AuthcryptEnvelopeSpec) : Prop :=
  env.ed25519SigValid = true
    ∧ env.mlDsa65SigValid = true
    ∧ env.decrypts = true

/-- Full acceptance gate including runtime expiry rejection. -/
def acceptsAuthcryptAt (now : Nat) (env : AuthcryptEnvelopeSpec) : Prop :=
  acceptsAuthcrypt env ∧ notExpiredAt now env.expiresTime

/-- Authcrypt acceptance requires both classical and post-quantum signatures. -/
theorem authcrypt_acceptance_requires_dual_signature
    (env : AuthcryptEnvelopeSpec)
    (h : acceptsAuthcrypt env) :
    env.ed25519SigValid = true ∧ env.mlDsa65SigValid = true := by
  exact ⟨h.1, h.2.1⟩

/-- Authcrypt acceptance also requires successful authenticated decryption. -/
theorem authcrypt_acceptance_requires_decrypt
    (env : AuthcryptEnvelopeSpec)
    (h : acceptsAuthcrypt env) :
    env.decrypts = true := by
  exact h.2.2

theorem authcrypt_acceptance_at_requires_not_expired
    (now : Nat)
    (env : AuthcryptEnvelopeSpec)
    (h : acceptsAuthcryptAt now env) :
    notExpiredAt now env.expiresTime := by
  exact h.2

/-- If either signature check fails, authcrypt acceptance is impossible. -/
theorem authcrypt_rejects_if_any_signature_invalid
    (env : AuthcryptEnvelopeSpec)
    (hEd : env.ed25519SigValid = false ∨ env.mlDsa65SigValid = false) :
    ¬ acceptsAuthcrypt env := by
  intro hAccept
  unfold acceptsAuthcrypt at hAccept
  rcases hEd with hBadEd | hBadPq
  · rw [hBadEd] at hAccept
    cases hAccept.1
  · rw [hBadPq] at hAccept
    cases hAccept.2.1

end Protocol
end Comms
end NucleusDB
end HeytingLean
