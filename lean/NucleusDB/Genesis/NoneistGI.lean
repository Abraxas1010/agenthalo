import Mathlib.Order.Closure
import NucleusDB.Genesis.Noneist

/-!
# NucleusDB.Genesis.NoneistGI

Galois-insertion surface for the ceremony closure operator.

This transfers the closure/Galois-insertion theorem family from the
NucleusPOD model into the concrete NucleusDB `CeremonyPhase` lattice.
-/

namespace HeytingLean
namespace NucleusDB
namespace Genesis

/-- Closure operator induced by the noneist ceremony nucleus. -/
abbrev ceremonyClosureGI : ClosureOperator CeremonyPhase :=
  R_nucleus.toClosureOperator

/-- Galois insertion of closed ceremony phases into the base lattice. -/
abbrev ceremonyGaloisInsertion :
    GaloisInsertion ceremonyClosureGI.toCloseds (↑) :=
  ceremonyClosureGI.gi

/-- If `y` is closed, closure over `x` lies below `y` exactly when `x ≤ y`. -/
theorem ceremony_closure_le_iff_le_of_closed {x y : CeremonyPhase}
    (hy : ceremonyClosureGI y = y) :
    ceremonyClosureGI x ≤ y ↔ x ≤ y := by
  have hyClosed : ceremonyClosureGI.IsClosed y :=
    (ceremonyClosureGI.isClosed_iff).2 hy
  exact hyClosed.closure_le_iff

end Genesis
end NucleusDB
end HeytingLean
