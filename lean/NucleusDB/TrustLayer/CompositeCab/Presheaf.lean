import NucleusDB.PaymentChannels.MultiChain.Graph

/-!
# NucleusDB.TrustLayer.CompositeCab.Presheaf

Presheaf-side data model for multi-chain compliance sections.
-/

namespace NucleusDB.TrustLayer.CompositeCab

open NucleusDB.PaymentChannels.MultiChain

/-- Chain-open "coverage" by a finite chain set. -/
structure ChainOpen where
  chains : Finset ChainId

/-- Local compliance section over one chain. -/
structure LocalComplianceSection where
  chain : ChainId
  agent : String
  compliant : Bool
  proofDigest : String
  deriving DecidableEq, Repr

/-- Presheaf carrier family for compliance sections. -/
abbrev CompliancePresheaf : Type 1 :=
  ChainId → Type

/-- Canonical compliance presheaf: each chain carries `LocalComplianceSection`. -/
def compliancePresheaf : CompliancePresheaf := fun _ => LocalComplianceSection

/-- Restriction map between opens (identity on section values, domain-constrained by inclusion). -/
def restrictSections
    (U V : ChainOpen) (_h : V.chains ⊆ U.chains)
    (σ : ChainId → LocalComplianceSection) : ChainId → LocalComplianceSection :=
  fun c => σ c

end NucleusDB.TrustLayer.CompositeCab
