import NucleusDB.Comms.Protocol.DIDCommSpec
import NucleusDB.Comms.Protocol.AnoncryptSpec
import NucleusDB.Comms.Privacy.FailClosedSpec
import NucleusDB.Comms.Privacy.NymLifecycleSpec

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

open Privacy

/-- Envelope kinds are intentionally a closed sum: no composition constructor exists. -/
inductive EnvelopeKind where
  | authcrypt
  | anoncrypt
  deriving DecidableEq, Repr

/-- Canonical runtime string for each envelope kind. -/
def envelopeKindTag : EnvelopeKind → String
  | .authcrypt => "authcrypt"
  | .anoncrypt => "anoncrypt"

/-- T26: valid envelope kinds are exhaustive and non-composed. -/
theorem envelope_kind_exhaustive (k : EnvelopeKind) :
    k = .authcrypt ∨ k = .anoncrypt := by
  cases k with
  | authcrypt => exact Or.inl rfl
  | anoncrypt => exact Or.inr rfl

theorem envelope_kind_tag_closed (tag : String) :
    (∃ k : EnvelopeKind, envelopeKindTag k = tag) ↔
      (tag = "authcrypt" ∨ tag = "anoncrypt") := by
  constructor
  · intro h
    rcases h with ⟨k, hk⟩
    cases k with
    | authcrypt =>
        left
        simpa [envelopeKindTag] using hk.symm
    | anoncrypt =>
        right
        simpa [envelopeKindTag] using hk.symm
  · intro h
    rcases h with hAuth | hAnon
    · refine ⟨EnvelopeKind.authcrypt, ?_⟩
      simpa [envelopeKindTag] using hAuth.symm
    · refine ⟨EnvelopeKind.anoncrypt, ?_⟩
      simpa [envelopeKindTag] using hAnon.symm

/-- Abstract single-layer ciphertext witness (no nested envelope layer). -/
axiom singleLayerCiphertext : EnvelopeKind → Prop

/-- Abstract ambiguity predicate from key-commitment analyses. -/
axiom decryptionAmbiguous : EnvelopeKind → Prop

/-- Under single-layer usage, decryption ambiguity does not arise. -/
axiom aes_gcm_single_layer_no_ambiguity :
    ∀ (k : EnvelopeKind), singleLayerCiphertext k → ¬ decryptionAmbiguous k

/-- Sender authentication at envelope layer (authcrypt signatures verified). -/
def senderAuthenticated (env : AuthcryptEnvelopeSpec) : Prop :=
  env.ed25519SigValid = true ∧ env.mlDsa65SigValid = true

/-- Transport anonymity is represented by maximum-privacy routing. -/
def transportAnonymous (level : PrivacyLevel) : Prop :=
  level = .maximum

/-- Authcrypt acceptance implies sender authentication; max routing implies transport anonymity. -/
theorem authcrypt_plus_nym_achieves_both
    (env : AuthcryptEnvelopeSpec)
    (privLevel : PrivacyLevel)
    (hAccept : acceptsAuthcrypt env)
    (hRoute : privLevel = .maximum) :
    senderAuthenticated env ∧ transportAnonymous privLevel := by
  have hDual := authcrypt_acceptance_requires_dual_signature env hAccept
  exact ⟨hDual, hRoute⟩

/-- Healthy Nym mode cannot silently degrade directly to disabled state. -/
theorem healthy_nym_preserves_non_disabled :
    nymTransition NymState.healthy ≠ NymState.disabled := by
  intro hEq
  have hAllowed := nym_healthy_no_disable
  rcases hAllowed with hHealthy | hUnhealthy
  · rw [hHealthy] at hEq
    cases hEq
  · rw [hUnhealthy] at hEq
    cases hEq

end Protocol
end Comms
end NucleusDB
end HeytingLean
