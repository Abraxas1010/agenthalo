import NucleusDB.Sheaf.ChainGluing
import NucleusDB.Sheaf.Coherence
import NucleusDB.TrustLayer.CompositeCab.GlobalCab

/-!
# NucleusDB.TrustLayer.CompositeCab.SheafBridge

Bridge from compliance sections to chain-transport gluing and coherence witnesses.
-/

namespace NucleusDB.TrustLayer.CompositeCab

open NucleusDB.PaymentChannels.MultiChain
open NucleusDB.Sheaf

/-- Bridge alias to the existing coherence witness surface. -/
abbrev CoherenceWitness :=
  HeytingLean.NucleusDB.Sheaf.CoherenceWitness

/-- Chain-indexed compliance carrier used by sheaf transport. -/
abbrev ComplianceCarrier (_ : ChainId) : Type := LocalComplianceSection

/-- Identity transport on local compliance sections across chains. -/
def complianceTransport : ChainTransport ComplianceCarrier LocalComplianceSection :=
  { toShared := fun _ x => x
    fromShared := fun _ x => x
    rt1 := by
      intro _chain x
      rfl
    rt2 := by
      intro _chain x
      rfl }

theorem compliance_gluing_implies_chain_gluing
    (c1 c2 : ChainId) (x : LocalComplianceSection) (y : LocalComplianceSection)
    (h : x = y) :
    ChainGluingCondition complianceTransport c1 c2 x y := by
  simpa [ChainGluingCondition, complianceTransport] using h

/-- Assemble a global CAB from a coherent family of local compliance sections. -/
noncomputable def assembleGlobalCAB
    (covered : Finset ChainId)
    (σ : ChainId → LocalComplianceSection)
    (hLocal : ∀ c, c ∈ covered → (σ c).compliant = true)
    (coh : CoherenceWitness LocalComplianceSection) : CompositeCab :=
  composite_cab_existence covered σ hLocal coh.digest

end NucleusDB.TrustLayer.CompositeCab
