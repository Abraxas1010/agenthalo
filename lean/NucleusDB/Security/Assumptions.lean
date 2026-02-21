namespace HeytingLean
namespace NucleusDB
namespace Security

/-- Commitment profile selected by the runtime. -/
inductive CommitmentBackend
  | ipa
  | kzg
  deriving DecidableEq, Repr

/-- Explicit cryptographic assumptions for NucleusDB security claims. -/
inductive Assumption
  | collisionResistance
  | witnessUnforgeability
  | ctAppendOnly
  | ipaBinding
  | kzgTrustedSetup
  deriving DecidableEq, Repr

/-- Security profile advertised by a deployment. -/
structure SecurityProfile where
  backend : CommitmentBackend
  assumptions : List Assumption

/-- KZG deployments require the trusted-setup assumption. -/
def SecurityProfile.WellFormed (p : SecurityProfile) : Prop :=
  match p.backend with
  | .ipa => True
  | .kzg => Assumption.kzgTrustedSetup ∈ p.assumptions

theorem kzg_wellFormed_requires_trustedSetup
    {p : SecurityProfile}
    (h : p.WellFormed)
    (hk : p.backend = CommitmentBackend.kzg) :
    Assumption.kzgTrustedSetup ∈ p.assumptions := by
  cases p with
  | mk backend assumptions =>
      cases hk
      simpa [SecurityProfile.WellFormed] using h

theorem ipa_wellFormed_trivial
    (assumptions : List Assumption) :
    (SecurityProfile.mk CommitmentBackend.ipa assumptions).WellFormed := by
  simp [SecurityProfile.WellFormed]

end Security
end NucleusDB
end HeytingLean
