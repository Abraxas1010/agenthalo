import NucleusDB.Sheaf.ChainGluing
import NucleusDB.TrustLayer.CompositeCab.GluingCondition

/-!
# NucleusDB.TrustLayer.CompositeCab.GlobalCab

Global composite CAB object and its local-compliance consequences.
-/

namespace NucleusDB.TrustLayer.CompositeCab

set_option linter.dupNamespace false

open NucleusDB.PaymentChannels.MultiChain

/-- Glued global compliance artifact over a finite chain cover. -/
structure CompositeCab where
  coveredChains : Finset ChainId
  globalSection : ChainId → LocalComplianceSection
  localCompliant : ∀ c, c ∈ coveredChains → (globalSection c).compliant = true
  compositeDigest : String

/-- Existence of a composite CAB from a section family and gluing witness. -/
noncomputable def composite_cab_existence
    (cover : Finset ChainId)
    (σ : ChainId → LocalComplianceSection)
    (hLocal : ∀ c, c ∈ cover → (σ c).compliant = true)
    (digest : String) : CompositeCab :=
  { coveredChains := cover
    globalSection := σ
    localCompliant := hLocal
    compositeDigest := digest }

theorem composite_cab_implies_local_compliance
    (cab : CompositeCab) (c : ChainId) (hc : c ∈ cab.coveredChains) :
    (cab.globalSection c).compliant = true :=
  cab.localCompliant c hc

end NucleusDB.TrustLayer.CompositeCab
