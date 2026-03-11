import Mathlib

namespace HeytingLean
namespace NucleusDB
namespace Core

/-!
Arithmetic mirror of the runtime trust diode.

This local model is intentionally lightweight: it proves the floor property used
by the Rust implementation, while the canonical algebraic justification lives in
`HeytingLean.EpistemicCalculus.NucleusBridge`.
-/

/-- Runtime-shaped epistemic trust carrier on `[0, 1]`, modelled in rationals. -/
structure EpistemicTrust where
  floor : Rat
  deriving Repr

def nucleus (t : EpistemicTrust) (x : Rat) : Rat :=
  max x t.floor

def fuse (x y : Rat) : Rat :=
  x * y

def combine (t : EpistemicTrust) (values : List Rat) : Rat :=
  nucleus t (values.foldl (fun acc value => fuse acc value) 1)

/-- The diode floor is preserved by `combine`. -/
theorem combine_floor_respected
    (t : EpistemicTrust) (values : List Rat) :
    t.floor ≤ combine t values := by
  unfold combine nucleus
  exact le_max_right _ _

end Core
end NucleusDB
end HeytingLean
