import NucleusDB.Sheaf.Coherence

namespace HeytingLean
namespace NucleusDB
namespace Sheaf

universe u v w

/-- Abstract materialization map plus transport-compatibility law. -/
structure MaterializationFunctor (State : Type u) (Idx : Type v) (Val : Type w) where
  toVector : State → Idx → Val
  transports : State → State → Prop
  naturality : ∀ s t, transports s t → toVector s = toVector t

/-- Naturality can be used directly as extensional equality after transport. -/
theorem materialize_transport_eq
    {State : Type u} {Idx : Type v} {Val : Type w}
    (M : MaterializationFunctor State Idx Val)
    {s t : State}
    (h : M.transports s t) :
    M.toVector s = M.toVector t :=
  M.naturality s t h

end Sheaf
end NucleusDB
end HeytingLean
