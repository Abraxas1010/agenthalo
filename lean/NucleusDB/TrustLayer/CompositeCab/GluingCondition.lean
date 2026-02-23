import NucleusDB.TrustLayer.CompositeCab.Presheaf

/-!
# NucleusDB.TrustLayer.CompositeCab.GluingCondition

Agreement/cocycle predicates for composite CAB section families.
-/

namespace NucleusDB.TrustLayer.CompositeCab

open NucleusDB.PaymentChannels.MultiChain

/-- Two section families agree on overlap. -/
def sectionsAgreeOnOverlap
    (U V : ChainOpen)
    (σ τ : ChainId → LocalComplianceSection) : Prop :=
  ∀ c, c ∈ U.chains ∩ V.chains → σ c = τ c

/-- Pairwise gluing condition for a family of opens and section assignments. -/
def gluingConditionHolds
    (opens : Finset ChainOpen)
    (sections : ChainOpen → ChainId → LocalComplianceSection) : Prop :=
  ∀ U ∈ opens, ∀ V ∈ opens, sectionsAgreeOnOverlap U V (sections U) (sections V)

/-- Triple-overlap cocycle condition induced by pairwise overlap agreement. -/
theorem cocycleCondition
    (U V W : ChainOpen)
    (σ τ υ : ChainId → LocalComplianceSection)
    (hUVAgree : sectionsAgreeOnOverlap U V σ τ)
    (hVWAgree : sectionsAgreeOnOverlap V W τ υ) :
    ∀ c, c ∈ U.chains ∩ V.chains ∩ W.chains → σ c = υ c := by
  intro c hc
  have hUVW : c ∈ (U.chains ∩ V.chains) ∩ W.chains := by
    simpa [Finset.inter_assoc] using hc
  have hUVMem : c ∈ U.chains ∩ V.chains := (Finset.mem_inter.mp hUVW).1
  have hVWMem : c ∈ V.chains ∩ W.chains := by
    refine Finset.mem_inter.mpr ?_
    refine ⟨(Finset.mem_inter.mp hUVMem).2, (Finset.mem_inter.mp hUVW).2⟩
  have hcUV : c ∈ U.chains ∩ V.chains := by
    exact hUVMem
  exact (hUVAgree c hcUV).trans (hVWAgree c hVWMem)

end NucleusDB.TrustLayer.CompositeCab
