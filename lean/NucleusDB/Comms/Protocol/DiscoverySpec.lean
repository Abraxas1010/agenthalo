namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

/-- Minimal verified announcement model used by discovery proofs. -/
structure AnnouncementSpec where
  did : String
  didDocumentId : String
  didKeyEd25519 : Nat
  documentEd25519 : Nat
  ed25519SigValid : Bool
  mlDsa65SigValid : Bool
  timestamp : Nat
  ttl : Nat
  deriving DecidableEq, Repr

/-- DID document binding check: identifier and Ed25519 key must match `did:key`. -/
def didDocumentBindingValid (a : AnnouncementSpec) : Prop :=
  a.didDocumentId = a.did ∧ a.didKeyEd25519 = a.documentEd25519

/-- Signature gate shape used by verified gossip/KAD ingestion. -/
def announcementSignaturesValid (a : AnnouncementSpec) : Prop :=
  a.ed25519SigValid = true ∧ a.mlDsa65SigValid = true

/-- Full verification predicate before an announcement is accepted. -/
def verifyAnnouncement (a : AnnouncementSpec) : Prop :=
  didDocumentBindingValid a ∧ announcementSignaturesValid a

theorem verify_requires_document_binding
    (a : AnnouncementSpec) (h : verifyAnnouncement a) :
    didDocumentBindingValid a := by
  exact h.1

theorem verify_requires_dual_signature
    (a : AnnouncementSpec) (h : verifyAnnouncement a) :
    announcementSignaturesValid a := by
  exact h.2

/-- Expiry predicate matching runtime `now > timestamp + ttl` pruning rule. -/
def isExpired (now : Nat) (a : AnnouncementSpec) : Prop :=
  now > a.timestamp + a.ttl

/-- Retained announcements after prune are not expired. -/
theorem retained_after_prune_not_expired
    (now : Nat) (a : AnnouncementSpec) :
    ¬ isExpired now a ↔ now ≤ a.timestamp + a.ttl := by
  unfold isExpired
  exact Nat.not_lt

end Protocol
end Comms
end NucleusDB
end HeytingLean
