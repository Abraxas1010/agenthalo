import NucleusDB.Sheaf.MaterializationFunctor
import NucleusDB.Identity.Materialization

namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v w

/-- Generic bridge: pointwise-equal materializations induce a natural transformation. -/
def materializationBridgeNat
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M : Sheaf.MaterializationFunctor State Idx Val) :
    Sheaf.materializationDiscreteFunctor M ⟶ Sheaf.materializationDiscreteFunctor M :=
  Sheaf.materializationIdentityNat M

/-- Concrete witness for the identity subsystem: self-map natural transformation. -/
def identityMaterializationIdNat :
    Identity.identityDiscreteMaterializationFunctor ⟶
      Identity.identityDiscreteMaterializationFunctor :=
  materializationBridgeNat Identity.identityMaterializationFunctor

end Core
end NucleusDB
end HeytingLean
