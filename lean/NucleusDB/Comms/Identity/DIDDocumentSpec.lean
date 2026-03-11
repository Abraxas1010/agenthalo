namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Identity

inductive VerificationClass where
  | classical
  | postQuantum
  deriving DecidableEq, Repr

inductive AgreementClass where
  | classical
  | postQuantum
  deriving DecidableEq, Repr

structure VerificationMethodSpec where
  id : String
  controller : String
  cls : VerificationClass
  deriving DecidableEq, Repr

structure KeyAgreementMethodSpec where
  id : String
  controller : String
  cls : AgreementClass
  deriving DecidableEq, Repr

structure DIDDocumentSpec where
  subject : String
  verificationMethods : List VerificationMethodSpec
  keyAgreementMethods : List KeyAgreementMethodSpec
  authenticationRefs : List String
  deriving DecidableEq, Repr

def uniqueStrings (xs : List String) : Bool :=
  decide xs.Nodup

def verificationIds (doc : DIDDocumentSpec) : List String :=
  doc.verificationMethods.map (·.id)

def keyAgreementIds (doc : DIDDocumentSpec) : List String :=
  doc.keyAgreementMethods.map (·.id)

def allControllersSelf (doc : DIDDocumentSpec) : Bool :=
  doc.verificationMethods.all (fun vm => vm.controller == doc.subject)
    && doc.keyAgreementMethods.all (fun km => km.controller == doc.subject)

def hasVerificationClass (doc : DIDDocumentSpec) (cls : VerificationClass) : Bool :=
  doc.verificationMethods.any (fun vm => vm.cls == cls)

def hasAgreementClass (doc : DIDDocumentSpec) (cls : AgreementClass) : Bool :=
  doc.keyAgreementMethods.any (fun km => km.cls == cls)

def authenticationRefsResolve (doc : DIDDocumentSpec) : Bool :=
  doc.authenticationRefs.all (fun ref => (verificationIds doc).contains ref)

def WellFormed (doc : DIDDocumentSpec) : Bool :=
  uniqueStrings (verificationIds doc)
    && uniqueStrings (keyAgreementIds doc)
    && allControllersSelf doc
    && hasVerificationClass doc .classical
    && hasVerificationClass doc .postQuantum
    && hasAgreementClass doc .classical
    && hasAgreementClass doc .postQuantum
    && authenticationRefsResolve doc

abbrev DIDSeed64 := Fin 64 → Nat

/-- Phase 0 identity seed projection (abstracted). -/
def deriveIdentity (_ : DIDSeed64) : String :=
  "did:key:z6Mk-nucleusdb-phase0"

/-- Runtime-shape DID document builder with dual classical + post-quantum methods. -/
def buildDIDDocument (subject : String) : DIDDocumentSpec :=
  { subject := subject
    verificationMethods :=
      [ { id := subject ++ "#key-ed25519-1", controller := subject, cls := .classical }
      , { id := subject ++ "#key-mldsa65-1", controller := subject, cls := .postQuantum }
      ]
    keyAgreementMethods :=
      [ { id := subject ++ "#key-x25519-1", controller := subject, cls := .classical }
      , { id := subject ++ "#key-mlkem768-1", controller := subject, cls := .postQuantum }
      ]
    authenticationRefs := [subject ++ "#key-ed25519-1", subject ++ "#key-mldsa65-1"] }

/-- Seed-indexed DID document constructor used by the theorem statement. -/
def buildDIDDocumentFromSeed (seed : DIDSeed64) : DIDDocumentSpec :=
  buildDIDDocument (deriveIdentity seed)

/-- T7: DID documents built from genesis-derived identity satisfy well-formedness checks. -/
theorem did_document_wellformed (seed : DIDSeed64) :
    WellFormed (buildDIDDocumentFromSeed seed) = true := by
  simp [WellFormed, buildDIDDocumentFromSeed, buildDIDDocument, deriveIdentity,
    uniqueStrings, verificationIds, keyAgreementIds, allControllersSelf,
    hasVerificationClass, hasAgreementClass, authenticationRefsResolve]

end Identity
end Comms
end NucleusDB
end HeytingLean
