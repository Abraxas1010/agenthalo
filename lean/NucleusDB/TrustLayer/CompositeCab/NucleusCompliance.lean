import Mathlib.Order.Nucleus
import NucleusDB.PaymentChannels.MultiChain.Graph

/-!
# NucleusDB.TrustLayer.CompositeCab.NucleusCompliance

Verification nucleus over compliance states using Mathlib's `Order.Nucleus`.
-/

namespace NucleusDB.TrustLayer.CompositeCab

open NucleusDB.PaymentChannels.MultiChain

/-- Compliance state as the set of chains currently verified for an agent. -/
abbrev ComplianceState : Type := Set ChainId

/-- Required baseline chains for `ΩR` verification. -/
def omegaRequiredChains : ComplianceState :=
  { c | c = ChainId.base ∨ c = ChainId.ethereum }

/-- Verification closure map: add required chains to any state. -/
def verificationMap (s : ComplianceState) : ComplianceState :=
  s ∪ omegaRequiredChains

/-- Mathlib nucleus capturing "closed under required-chain verification". -/
def verificationNucleus : Nucleus ComplianceState :=
  { toFun := verificationMap
    map_inf' := by
      intro s t
      ext c
      constructor
      · intro hc
        rcases hc with hst | hR
        · exact ⟨Or.inl hst.1, Or.inl hst.2⟩
        · exact ⟨Or.inr hR, Or.inr hR⟩
      · intro hc
        rcases hc with ⟨hsR, htR⟩
        rcases hsR with hs | hR
        · rcases htR with ht | hR'
          · exact Or.inl ⟨hs, ht⟩
          · exact Or.inr hR'
        · exact Or.inr hR
    le_apply' := by
      intro s c hc
      exact Or.inl hc
    idempotent' := by
      intro s c hc
      rcases hc with hc | hc
      · exact hc
      · exact Or.inr hc }

/-- `ΩR` is fixed by the verification nucleus. -/
theorem omega_R_is_verified_states :
    verificationNucleus omegaRequiredChains = omegaRequiredChains := by
  ext c
  constructor
  · intro hc
    rcases hc with hc | hc
    · exact hc
    · exact hc
  · intro hc
    exact Or.inr hc

end NucleusDB.TrustLayer.CompositeCab
