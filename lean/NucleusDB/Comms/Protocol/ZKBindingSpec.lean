namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

inductive CredentialMessageKind where
  | offer
  | request
  | issue
  | other
  deriving DecidableEq, Repr

/-- Minimal credential attachment witness carried in DIDComm credential flows. -/
structure CredentialAttachmentSpec where
  proofSchemaVersion : Nat
  proofVerifies : Bool
  resourceUriPresent : Bool
  requestedActionPresent : Bool
  deriving DecidableEq, Repr

/-- DIDComm/ZK binding gate used by credential handlers. -/
def credentialBindingAccepts
    (kind : CredentialMessageKind)
    (attachment? : Option CredentialAttachmentSpec) : Prop :=
  match kind, attachment? with
  | .request, some att =>
      att.proofSchemaVersion = 2
        ∧ att.proofVerifies = true
        ∧ att.resourceUriPresent = true
        ∧ att.requestedActionPresent = true
  | .issue, some att =>
      att.proofSchemaVersion = 2
        ∧ att.resourceUriPresent = true
        ∧ att.requestedActionPresent = true
  | .offer, _ => True
  | .other, _ => True
  | _, none => False

theorem credential_request_requires_verified_proof
    (attachment : CredentialAttachmentSpec)
    (h : credentialBindingAccepts .request (some attachment)) :
    attachment.proofVerifies = true := by
  exact h.2.1

theorem credential_request_without_attachment_rejected :
    ¬ credentialBindingAccepts .request none := by
  unfold credentialBindingAccepts
  simp

end Protocol
end Comms
end NucleusDB
end HeytingLean
