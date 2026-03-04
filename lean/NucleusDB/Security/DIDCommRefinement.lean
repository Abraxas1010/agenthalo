import NucleusDB.Comms.Protocol.DIDCommSpec
import NucleusDB.Comms.Protocol.AnoncryptSpec
import NucleusDB.Comms.Protocol.CompositionPolicy

namespace HeytingLean
namespace NucleusDB
namespace Security

open Comms.Protocol

/-- Abstract runtime gate projection for authcrypt acceptance in Rust. -/
structure RustAuthcryptGate where
  edOk : Bool
  pqOk : Bool
  decryptOk : Bool
  enrichmentPresent : Bool
  enrichmentSenderMatches : Bool
  expiresTime : Option Nat
  now : Nat
  deriving DecidableEq, Repr

/-- Rust-side decision function modeled in Lean. -/
def rustAcceptsAuthcrypt (g : RustAuthcryptGate) : Prop :=
  g.edOk = true
    ∧ g.pqOk = true
    ∧ g.decryptOk = true
    ∧ (g.enrichmentPresent = false ∨ g.enrichmentSenderMatches = true)
    ∧ notExpiredAt g.now g.expiresTime

/-- Refinement relation from runtime gate witness to protocol envelope spec. -/
def refinesAuthcryptGate (g : RustAuthcryptGate) (env : AuthcryptEnvelopeSpec) : Prop :=
  env.ed25519SigValid = g.edOk
    ∧ env.mlDsa65SigValid = g.pqOk
    ∧ env.decrypts = g.decryptOk
    ∧ (g.enrichmentPresent = false ∨ g.enrichmentSenderMatches = true)
    ∧ env.expiresTime = g.expiresTime

theorem rust_authcrypt_refines_protocol
    (g : RustAuthcryptGate) (env : AuthcryptEnvelopeSpec)
    (hRef : refinesAuthcryptGate g env) :
    rustAcceptsAuthcrypt g ↔ acceptsAuthcryptAt g.now env := by
  rcases hRef with ⟨hEd, hPq, hDec, hEnrich, hExp⟩
  constructor
  · intro h
    have hEd' : env.ed25519SigValid = true := by simpa [hEd] using h.1
    have hPq' : env.mlDsa65SigValid = true := by simpa [hPq] using h.2.1
    have hDec' : env.decrypts = true := by simpa [hDec] using h.2.2.1
    have hExp' : notExpiredAt g.now env.expiresTime := by simpa [hExp] using h.2.2.2.2
    exact ⟨⟨hEd', hPq', hDec'⟩, hExp'⟩
  · intro h
    rcases h with ⟨hGate, hNotExpired⟩
    have hEd' : g.edOk = true := by simpa [hEd] using hGate.1
    have hPq' : g.pqOk = true := by simpa [hPq] using hGate.2.1
    have hDec' : g.decryptOk = true := by simpa [hDec] using hGate.2.2
    have hEnrich' : g.enrichmentPresent = false ∨ g.enrichmentSenderMatches = true := hEnrich
    have hExp' : notExpiredAt g.now g.expiresTime := by simpa [hExp] using hNotExpired
    exact ⟨hEd', hPq', hDec', hEnrich', hExp'⟩

/-- Abstract runtime gate for anoncrypt acceptance in Rust. -/
structure RustAnoncryptGate where
  decryptOk : Bool
  expiresTime : Option Nat
  now : Nat
  deriving DecidableEq, Repr

def rustAcceptsAnoncrypt (g : RustAnoncryptGate) : Prop :=
  g.decryptOk = true ∧ anonNotExpiredAt g.now g.expiresTime

theorem rust_anoncrypt_refines_protocol
    (g : RustAnoncryptGate) :
    rustAcceptsAnoncrypt g ↔
      acceptsAnoncryptAt g.now {
        decrypts := g.decryptOk
        senderAuthenticated := false
        expiresTime := g.expiresTime
      } := by
  unfold rustAcceptsAnoncrypt acceptsAnoncryptAt
  rfl

/-- Runtime acceptability model for unpack-side envelope kind checks. -/
def rustUnpackAcceptsKind (kind : String) : Prop :=
  kind.contains '(' = false
    ∧ kind.contains ')' = false
    ∧ (kind = envelopeKindTag EnvelopeKind.authcrypt
       ∨ kind = envelopeKindTag EnvelopeKind.anoncrypt)

theorem rust_unpack_rejects_composition
    (kind : String)
    (hNested : kind.contains '(' = true ∨ kind.contains ')' = true) :
    ¬ rustUnpackAcceptsKind kind := by
  intro hAccept
  rcases hAccept with ⟨hLeft, hRight, _hValid⟩
  rcases hNested with hL | hR
  · rw [hL] at hLeft
    cases hLeft
  · rw [hR] at hRight
    cases hRight

theorem rust_unpack_accepts_iff_valid_kind
    (kind : String) :
    rustUnpackAcceptsKind kind ↔
      (kind = envelopeKindTag EnvelopeKind.authcrypt
        ∨ kind = envelopeKindTag EnvelopeKind.anoncrypt) := by
  constructor
  · intro h
    exact h.2.2
  · intro hValid
    refine ⟨?_, ?_, hValid⟩
    · rcases hValid with hAuth | hAnon
      · rw [hAuth]
        native_decide
      · rw [hAnon]
        native_decide
    · rcases hValid with hAuth | hAnon
      · rw [hAuth]
        native_decide
      · rw [hAnon]
        native_decide

end Security
end NucleusDB
end HeytingLean
