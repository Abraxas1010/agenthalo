import NucleusDB.Core.Certificates
import NucleusDB.Core.Ledger
import NucleusDB.Identity.Policy

namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Identity-specialized commit certificate alias. -/
abbrev IdentityCommitCertificate :=
  Core.CommitCertificate IdentityState IdentityDelta IdentityAuth applyDelta identityPolicy

/-- Identity-specialized commit record alias. -/
abbrev IdentityCommitRecord :=
  Core.CommitRecord IdentityState IdentityDelta IdentityAuth applyDelta identityPolicy

/-- Identity ledger verifier alias. -/
abbrev verifyIdentityLedger :=
  Core.verifyLedger
    (State := IdentityState)
    (Delta := IdentityDelta)
    (Auth := IdentityAuth)
    (apply := applyDelta)
    (policy := identityPolicy)

theorem verifyIdentityCommitCertificate_sound (c : IdentityCommitCertificate) :
    Core.verifyCommitCertificate c := by
  exact Core.verifyCommitCertificate_sound c

theorem verifyIdentityLedger_nil : verifyIdentityLedger [] := by
  exact Core.verifyLedger_nil

theorem verifyIdentityLedger_cons
    (r : IdentityCommitRecord) (rs : List IdentityCommitRecord) :
    verifyIdentityLedger (r :: rs) ↔
      Core.verifyCommitCertificate r.cert ∧ verifyIdentityLedger rs := by
  exact Core.verifyLedger_cons r rs

end Identity
end NucleusDB
end HeytingLean
