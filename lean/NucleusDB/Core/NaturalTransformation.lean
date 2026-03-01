import NucleusDB.Sheaf.MaterializationFunctor
import NucleusDB.Identity.Materialization

namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v w

/-- Generic bridge: pointwise-equal materializations induce a natural transformation. -/
def materializationBridgeNat
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M N : Sheaf.MaterializationFunctor State Idx Val)
    (hEq : ∀ s, M.toVector s = N.toVector s) :
    Sheaf.materializationDiscreteFunctor M ⟶ Sheaf.materializationDiscreteFunctor N where
  app s := CategoryTheory.Discrete.eqToHom (hEq s.as)
  naturality := by
    intro X Y f
    apply Subsingleton.elim

/-- Concrete witness for the identity subsystem: self-map natural transformation. -/
def identityMaterializationIdNat :
    Identity.identityDiscreteMaterializationFunctor ⟶
      Identity.identityDiscreteMaterializationFunctor :=
  materializationBridgeNat
    Identity.identityMaterializationFunctor
    Identity.identityMaterializationFunctor
    (fun _ => rfl)

end Core
end NucleusDB
end HeytingLean
