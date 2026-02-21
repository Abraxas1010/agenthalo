import NucleusDB.Security.Assumptions

namespace HeytingLean
namespace NucleusDB
namespace Security

/-- One reduction statement from a concrete claim to an assumption. -/
structure ReductionContract where
  claim : String
  assumption : Assumption
  lossBits : Nat
  maxQueries : Nat

/-- Minimal validity constraints for a reduction contract. -/
def ReductionContract.Valid (r : ReductionContract) : Prop :=
  0 < r.lossBits ∧ 0 < r.maxQueries

/-- Bundle-level validity for reduction disclosures. -/
def ReductionBundleValid (rs : List ReductionContract) : Prop :=
  rs ≠ [] ∧ ∀ r ∈ rs, r.Valid

theorem singleton_bundle_valid
    (r : ReductionContract)
    (hvalid : r.Valid) :
    ReductionBundleValid [r] := by
  constructor
  · simp
  · intro r' hr'
    simp at hr'
    cases hr'
    simp [*] at hvalid ⊢

theorem bundle_member_valid
    {rs : List ReductionContract}
    (h : ReductionBundleValid rs)
    {r : ReductionContract}
    (hr : r ∈ rs) :
    r.Valid :=
  by simpa using h.2 r hr

end Security
end NucleusDB
end HeytingLean
