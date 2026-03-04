namespace HeytingLean
namespace NucleusDB
namespace Security

/-- Trust tiers exposed by the proof gate. -/
inductive TrustTier where
  | untrusted
  | legacy
  | cryptoExtended
  | standard
  deriving DecidableEq, Repr

/-- Minimal certificate witness consumed by the gate model. -/
structure CertificateWitness where
  theoremNames : List String
  axiomsUsed : List String
  commitHash : Option String
  theoremStatementSha256 : Option String
  signatureValid : Option Bool
  deriving DecidableEq, Repr

/-- Tool requirement modeled in Lean. -/
structure GateRequirementSpec where
  theoremName : String
  enforced : Bool
  expectedStatementHash : Option String
  expectedCommitHash : Option String
  requireSignature : Bool
  minTrustTier : Option TrustTier
  deriving DecidableEq, Repr

def standardAxiomSet : List String :=
  ["propext", "Classical.choice", "Quot.sound"]

def trustedAxiomSet : List String :=
  standardAxiomSet ++ ["HeytingLean.NucleusDB.Comms.Identity.hkdf_is_prf"]

def theoremPresent (cert : CertificateWitness) (name : String) : Prop :=
  name ∈ cert.theoremNames

def axiomsTrusted (cert : CertificateWitness) : Bool :=
  cert.axiomsUsed.all (fun ax => ax ∈ trustedAxiomSet)

def usesOnlyStandardAxioms (cert : CertificateWitness) : Bool :=
  cert.axiomsUsed.all (fun ax => ax ∈ standardAxiomSet)

def trustTier (cert : CertificateWitness) : TrustTier :=
  if axiomsTrusted cert then
    if cert.commitHash.isNone && cert.theoremStatementSha256.isNone && cert.signatureValid.isNone then
      .legacy
    else if usesOnlyStandardAxioms cert then
      .standard
    else
      .cryptoExtended
  else
    .untrusted

def trustTierMeets (actual required : TrustTier) : Prop :=
  match required with
  | .standard => actual = .standard
  | .cryptoExtended => actual = .standard ∨ actual = .cryptoExtended
  | .legacy => actual = .legacy ∨ actual = .standard ∨ actual = .cryptoExtended
  | .untrusted => True

def requirementAccepts (req : GateRequirementSpec) (cert : CertificateWitness) : Prop :=
  theoremPresent cert req.theoremName
    ∧ axiomsTrusted cert = true
    ∧ (match req.expectedStatementHash with
      | none => True
      | some h => cert.theoremStatementSha256 = some h)
    ∧ (match req.expectedCommitHash with
      | none => True
      | some h => cert.commitHash = some h)
    ∧ (if req.requireSignature then cert.signatureValid = some true else True)
    ∧ (match req.minTrustTier with
      | none => True
      | some tier => trustTierMeets (trustTier cert) tier)

def gateEvaluate (requirements : List GateRequirementSpec) (certs : List CertificateWitness) : Prop :=
  ∀ req ∈ requirements, req.enforced = true → ∃ cert ∈ certs, requirementAccepts req cert

theorem standard_tier_implies_crypto_extended
    (t : TrustTier)
    (h : t = .standard) :
    trustTierMeets t .cryptoExtended := by
  subst h
  exact Or.inl rfl

end Security
end NucleusDB
end HeytingLean
