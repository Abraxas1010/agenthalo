namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v

/-- State invariant predicate. -/
def Invariant (State : Type u) : Type u :=
  State → Prop

/-- Transition preserves invariant if every valid step remains in the invariant. -/
def PreservedBy (State : Type u) (Delta : Type v)
    (apply : State → Delta → State) (inv : Invariant State) : Prop :=
  ∀ s d, inv s → inv (apply s d)

/-- Replay a list of deltas through `apply`. -/
def replay (State : Type u) (Delta : Type v)
    (apply : State → Delta → State) : State → List Delta → State
  | s, [] => s
  | s, d :: ds => replay State Delta apply (apply s d) ds

/-- If `apply` preserves `inv`, replay preserves `inv`. -/
theorem replay_preserves
    (State : Type u) (Delta : Type v)
    (apply : State → Delta → State)
    (inv : Invariant State)
    (hPres : PreservedBy State Delta apply inv) :
    ∀ s ds, inv s → inv (replay State Delta apply s ds) := by
  intro s ds hs
  induction ds generalizing s with
  | nil =>
      simpa [replay] using hs
  | cons d ds ih =>
      have hs' : inv (apply s d) := hPres s d hs
      simpa [replay] using ih (apply s d) hs'

end Core
end NucleusDB
end HeytingLean
