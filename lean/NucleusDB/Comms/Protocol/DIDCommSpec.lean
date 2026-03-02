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
  deriving DecidableEq, Repr

/-- Acceptance predicate matching the runtime `unpack_with_resolver` gate shape. -/
def acceptsAuthcrypt (env : AuthcryptEnvelopeSpec) : Prop :=
  env.ed25519SigValid = true
    ∧ env.mlDsa65SigValid = true
    ∧ env.decrypts = true

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
