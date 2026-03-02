import Mathlib.CategoryTheory.Types.Basic

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Identity

abbrev PublicKey := Nat

/-- Minimal did:key payload model: multicodec prefix + base58 payload witness. -/
structure DidKey where
  multicodecPrefix : Nat
  base58Payload : PublicKey
  deriving DecidableEq, Repr

/-- did:key generation model (multicodec prefix fixed at Ed25519 `0xed01`). -/
def didKeyEncode (pk : PublicKey) : DidKey :=
  { multicodecPrefix := 0xed01
    base58Payload := pk }

/-- Different public keys always map to different did:key identifiers. -/
theorem did_key_encode_injective : Function.Injective didKeyEncode := by
  intro a b h
  cases h
  rfl

structure Agent where
  publicKey : PublicKey
  deriving DecidableEq, Repr

structure DIDDocument where
  subject : DidKey
  deriving DecidableEq, Repr

/-- Lawvere-style primitive operation: key generation returns agent + DID document. -/
def keygen (pk : PublicKey) : PUnit → Agent × DIDDocument :=
  fun _ => ({ publicKey := pk }, { subject := didKeyEncode pk })

/-- Lawvere-style primitive operation: resolve agent to its DID document. -/
def resolve : Agent → DIDDocument :=
  fun agent => { subject := didKeyEncode agent.publicKey }

/-- Category-theoretic view in `Type`: operation morphisms. -/
def keygenHom (pk : PublicKey) : PUnit ⟶ (Agent × DIDDocument) :=
  keygen pk

def resolveHom : Agent ⟶ DIDDocument :=
  resolve

/-- T13 (partial): keygen/resolve law for DID identity consistency. -/
theorem didcomm_keygen_resolve_consistent (pk : PublicKey) :
    resolve ((keygen pk PUnit.unit).1) = (keygen pk PUnit.unit).2 := by
  rfl

end Identity
end Comms
end NucleusDB
end HeytingLean
