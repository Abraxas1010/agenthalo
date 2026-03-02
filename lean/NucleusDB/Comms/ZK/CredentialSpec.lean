namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace ZK

/-- Public credential statement visible to verifiers. -/
structure CredentialStatement where
  granteeDidHash : Nat
  keyPatternHash : Nat
  permissionFlags : Nat
  currentTime : Nat
  deriving DecidableEq, Repr

/-- Private credential witness kept by the prover. -/
structure CredentialWitness where
  grantIdHash : Nat
  grantorDidHash : Nat
  expiresAt : Nat
  createdAt : Nat
  grantPermissionsFull : Nat
  nonce : Nat
  deriving DecidableEq, Repr

/-- Bit-mask subset relation (`requested` is contained in `granted`). -/
def permissionSubset (requested granted : Nat) : Prop :=
  Nat.land requested granted = requested

/-- Credential relation used by the NucleusDB communication layer. -/
def credentialRel (stmt : CredentialStatement) (wit : CredentialWitness) : Prop :=
  (wit.expiresAt = 0 ∨ stmt.currentTime < wit.expiresAt)
    ∧ permissionSubset stmt.permissionFlags wit.grantPermissionsFull
    ∧ stmt.permissionFlags > 0

/-- T17: valid unexpired credentials satisfy the relation. -/
theorem credential_completeness
    (stmt : CredentialStatement) (wit : CredentialWitness)
    (hExpiry : wit.expiresAt = 0 ∨ stmt.currentTime < wit.expiresAt)
    (hPerm : permissionSubset stmt.permissionFlags wit.grantPermissionsFull)
    (hNontrivial : stmt.permissionFlags > 0) :
    credentialRel stmt wit := by
  exact ⟨hExpiry, hPerm, hNontrivial⟩

/-- T18: satisfying the relation implies permission-subset soundness. -/
theorem credential_soundness
    (stmt : CredentialStatement) (wit : CredentialWitness)
    (h : credentialRel stmt wit) :
    permissionSubset stmt.permissionFlags wit.grantPermissionsFull :=
  h.2.1

end ZK
end Comms
end NucleusDB
end HeytingLean
