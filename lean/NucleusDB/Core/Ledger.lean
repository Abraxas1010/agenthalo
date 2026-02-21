import NucleusDB.Core.Certificates
import NucleusDB.Core.Invariants

namespace HeytingLean
namespace NucleusDB
namespace Core

universe u v w

/-- Ledger record for a single commit transition. -/
structure CommitRecord (State : Type u) (Delta : Type v) (Auth : Type w)
    (apply : State → Delta → State) (policy : AuthorizationPolicy State Delta Auth) where
  cert : CommitCertificate State Delta Auth apply policy

/-- Verify every record in a ledger sequence. -/
def verifyLedger
    {State : Type u} {Delta : Type v} {Auth : Type w}
    {apply : State → Delta → State}
    {policy : AuthorizationPolicy State Delta Auth} :
    List (CommitRecord State Delta Auth apply policy) → Prop
  | [] => True
  | r :: rs => verifyCommitCertificate r.cert ∧ verifyLedger rs

theorem verifyLedger_nil
    {State : Type u} {Delta : Type v} {Auth : Type w}
    {apply : State → Delta → State}
    {policy : AuthorizationPolicy State Delta Auth} :
    verifyLedger (State := State) (Delta := Delta) (Auth := Auth)
      (apply := apply) (policy := policy) [] :=
  trivial

theorem verifyLedger_cons
    {State : Type u} {Delta : Type v} {Auth : Type w}
    {apply : State → Delta → State}
    {policy : AuthorizationPolicy State Delta Auth}
    (r : CommitRecord State Delta Auth apply policy)
    (rs : List (CommitRecord State Delta Auth apply policy)) :
    verifyLedger (r :: rs) ↔ verifyCommitCertificate r.cert ∧ verifyLedger rs :=
  Iff.rfl

end Core
end NucleusDB
end HeytingLean
