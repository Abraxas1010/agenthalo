namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

/-- Hierarchical capability domain used for capability-addressed routing. -/
structure CapabilityDomain where
  path : String
  schemaVersion : Nat
  deriving DecidableEq, Repr

/-- Capability query is prefix-based: `prove/lean` matches `prove/lean/algebra`. -/
def domainMatchesPrefix (domain : CapabilityDomain) (pfx : String) : Prop :=
  domain.path = pfx ∨ ∃ suffix, domain.path = pfx ++ "/" ++ suffix

/-- Minimal attestation witness for formal freshness checks. -/
structure CapabilityAttestationSpec where
  attesterDid : String
  subjectDid : String
  capabilityId : String
  passed : Bool
  verifiedAt : Nat
  deriving DecidableEq, Repr

/-- A capability is considered freshly attested if it passed and has not expired. -/
def attestationFresh (now maxAge : Nat) (att : CapabilityAttestationSpec) : Prop :=
  att.passed = true ∧ now ≤ att.verifiedAt + maxAge

/-- Minimal capability query for soundness statements. -/
structure CapabilityQuery where
  domainPrefix : String
  requiredConstraints : List String
  deriving DecidableEq, Repr

/-- Minimal capability spec for query soundness statements. -/
structure CapabilitySpec where
  domain : CapabilityDomain
  constraints : List String
  deriving DecidableEq, Repr

def allConstraintsMet (spec : CapabilitySpec) (required : List String) : Prop :=
  ∀ c, c ∈ required → c ∈ spec.constraints

def specSatisfiesQuery (spec : CapabilitySpec) (q : CapabilityQuery) : Prop :=
  domainMatchesPrefix spec.domain q.domainPrefix ∧
  allConstraintsMet spec q.requiredConstraints

/-- Prefix soundness: exact domain path always matches itself. -/
theorem domain_prefix_reflexive (domain : CapabilityDomain) :
    domainMatchesPrefix domain domain.path := by
  exact Or.inl rfl

/-- Fresh attestations were verified no more than `maxAge` time units ago. -/
theorem fresh_attestation_bounded_age
    (now maxAge : Nat) (att : CapabilityAttestationSpec)
    (hFresh : attestationFresh now maxAge att) :
    now ≤ att.verifiedAt + maxAge := by
  exact hFresh.2

/-- Query matching is sound with respect to required constraints. -/
theorem query_match_sound (q : CapabilityQuery) (spec : CapabilitySpec) :
    specSatisfiesQuery spec q → allConstraintsMet spec q.requiredConstraints := by
  intro h
  exact h.2

end Protocol
end Comms
end NucleusDB
end HeytingLean
