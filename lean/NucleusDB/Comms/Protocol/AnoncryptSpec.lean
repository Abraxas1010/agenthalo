namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

/-- Minimal anoncrypt envelope model: no sender authentication requirement. -/
structure AnoncryptEnvelopeSpec where
  decrypts : Bool
  senderAuthenticated : Bool
  expiresTime : Option Nat
  deriving DecidableEq, Repr

/-- Runtime expiry gate shared with authcrypt. -/
def anonNotExpiredAt (now : Nat) (expiresTime : Option Nat) : Prop :=
  match expiresTime with
  | none => True
  | some expires => now ≤ expires

/-- Anoncrypt acceptance matches runtime behavior: decrypt success + not expired. -/
def acceptsAnoncryptAt (now : Nat) (env : AnoncryptEnvelopeSpec) : Prop :=
  env.decrypts = true ∧ anonNotExpiredAt now env.expiresTime

theorem anoncrypt_acceptance_requires_decrypt
    (now : Nat) (env : AnoncryptEnvelopeSpec)
    (h : acceptsAnoncryptAt now env) :
    env.decrypts = true := by
  exact h.1

/-- Sender-authentication bit is semantically irrelevant for anoncrypt acceptance. -/
theorem anoncrypt_sender_auth_irrelevant
    (now : Nat)
    (env : AnoncryptEnvelopeSpec)
    (a b : Bool) :
    acceptsAnoncryptAt now { env with senderAuthenticated := a } ↔
      acceptsAnoncryptAt now { env with senderAuthenticated := b } := by
  unfold acceptsAnoncryptAt
  rfl

end Protocol
end Comms
end NucleusDB
end HeytingLean
