namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v

/-- Minimal nucleus-style transition interface for NucleusDB state evolution. -/
structure NucleusSystem where
  State : Type u
  Delta : Type v
  apply : State → Delta → State

namespace NucleusSystem

/-- One transition step in the nucleus system. -/
def step (S : NucleusSystem) (s : S.State) (d : S.Delta) : S.State :=
  S.apply s d

@[simp] theorem step_eq_apply (S : NucleusSystem) (s : S.State) (d : S.Delta) :
    S.step s d = S.apply s d :=
  rfl

end NucleusSystem

end Core
end NucleusDB
end HeytingLean
