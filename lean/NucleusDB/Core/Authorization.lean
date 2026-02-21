namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v w

/-- Authorization predicate for applying a delta to a state under credential `Auth`. -/
def AuthorizationPolicy (State : Type u) (Delta : Type v) (Auth : Type w) : Type (max u v w) :=
  State → Delta → Auth → Prop

/-- A delta packaged with an authorization witness. -/
structure AuthorizedDelta (State : Type u) (Delta : Type v) (Auth : Type w)
    (policy : AuthorizationPolicy State Delta Auth) where
  state : State
  delta : Delta
  auth : Auth
  authorized : policy state delta auth

end Core
end NucleusDB
end HeytingLean
