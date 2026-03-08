namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Mesh

/-- A slot is satisfied when at least one provider has been assigned. -/
structure TaskSlot where
  slotId : String
  required : Bool
  assignedProviders : List String
  deriving DecidableEq, Repr

/-- Atomic formation means every required slot is satisfied together. -/
def slotSatisfied (slot : TaskSlot) : Prop :=
  slot.required = false ∨ slot.assignedProviders ≠ []

/-- A task formation is atomic when all required slots are satisfied. -/
def formationAtomic (slots : List TaskSlot) : Prop :=
  ∀ slot, slot ∈ slots → slotSatisfied slot

/-- If any required slot lacks providers, the formation is not atomic. -/
theorem missing_required_slot_breaks_atomicity
    (slots : List TaskSlot) (slot : TaskSlot)
    (hMem : slot ∈ slots)
    (hReq : slot.required = true)
    (hEmpty : slot.assignedProviders = []) :
    ¬ formationAtomic slots := by
  intro hAtomic
  have hSatisfied := hAtomic slot hMem
  unfold slotSatisfied at hSatisfied
  cases hSatisfied with
  | inl hFalse =>
      simp [hReq] at hFalse
  | inr hNe =>
      exact hNe hEmpty

/-- Adding an assignment to every required slot yields an atomic formation. -/
theorem atomic_when_all_required_slots_assigned
    (slots : List TaskSlot)
    (hAll : ∀ slot, slot ∈ slots → slot.required = true → slot.assignedProviders ≠ []) :
    formationAtomic slots := by
  intro slot hMem
  unfold slotSatisfied
  by_cases hReq : slot.required = true
  · exact Or.inr (hAll slot hMem hReq)
  · cases hFalse : slot.required with
    | false =>
        simp
    | true =>
        exact False.elim (hReq hFalse)

end Mesh
end Comms
end NucleusDB
end HeytingLean
