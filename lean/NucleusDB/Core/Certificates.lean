import NucleusDB.Core.Authorization
import NucleusDB.Core.Nucleus

namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v w

/-- Commit certificate for one nucleus transition. -/
structure CommitCertificate (State : Type u) (Delta : Type v) (Auth : Type w)
    (apply : State → Delta → State) (policy : AuthorizationPolicy State Delta Auth) where
  prev : State
  delta : Delta
  auth : Auth
  next : State
  authOk : policy prev delta auth
  nextEq : next = apply prev delta

/-- Certificate verifier predicate. -/
def verifyCommitCertificate
    {State : Type u} {Delta : Type v} {Auth : Type w}
    {apply : State → Delta → State}
    {policy : AuthorizationPolicy State Delta Auth}
    (c : CommitCertificate State Delta Auth apply policy) : Prop :=
  policy c.prev c.delta c.auth ∧ c.next = apply c.prev c.delta

theorem verifyCommitCertificate_sound
    {State : Type u} {Delta : Type v} {Auth : Type w}
    {apply : State → Delta → State}
    {policy : AuthorizationPolicy State Delta Auth}
    (c : CommitCertificate State Delta Auth apply policy) :
    verifyCommitCertificate c := by
  exact ⟨c.authOk, c.nextEq⟩

end Core
end NucleusDB
end HeytingLean
