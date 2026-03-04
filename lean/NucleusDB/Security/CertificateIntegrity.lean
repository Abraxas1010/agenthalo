namespace HeytingLean
namespace NucleusDB
namespace Security

/-- Signed certificate envelope abstraction. -/
structure CertificateEnvelope where
  commitHash : String
  theoremStatementSha256 : String
  generatedAt : Nat
  signerDid : String
  contentHash : String
  signatureValid : Bool
  deriving DecidableEq, Repr

/-- Requirement-side freshness binding witness. -/
structure CertificateFreshnessRequirement where
  expectedCommitHash : Option String
  expectedStatementSha256 : Option String
  requireSignature : Bool
  deriving DecidableEq, Repr

/-- Acceptance predicate used by the gate for envelope integrity. -/
def envelopeAccepts
    (req : CertificateFreshnessRequirement)
    (env : CertificateEnvelope) : Prop :=
  (match req.expectedCommitHash with
    | none => True
    | some h => env.commitHash = h)
    ∧ (match req.expectedStatementSha256 with
      | none => True
      | some h => env.theoremStatementSha256 = h)
    ∧ (if req.requireSignature then env.signatureValid = true else True)

/-- Content-addressing determinism: equal envelopes have equal content hashes. -/
theorem certificate_content_addressed
    (a b : CertificateEnvelope)
    (h : a = b) :
    a.contentHash = b.contentHash := by
  cases h
  rfl

/-- Signature binding: if envelope requires signature and accepts, signature is valid. -/
theorem certificate_signature_binding
    (req : CertificateFreshnessRequirement)
    (env : CertificateEnvelope)
    (hReq : req.requireSignature = true)
    (h : envelopeAccepts req env) :
    env.signatureValid = true := by
  unfold envelopeAccepts at h
  simpa [hReq] using h.2.2

/-- Stale certificates are rejected when statement hash does not match. -/
theorem stale_certificate_rejected
    (env : CertificateEnvelope)
    (expected : String)
    (hMismatch : env.theoremStatementSha256 ≠ expected) :
    ¬ envelopeAccepts
      { expectedCommitHash := none
        expectedStatementSha256 := some expected
        requireSignature := false }
      env := by
  intro h
  unfold envelopeAccepts at h
  exact hMismatch h.2.1

end Security
end NucleusDB
end HeytingLean
