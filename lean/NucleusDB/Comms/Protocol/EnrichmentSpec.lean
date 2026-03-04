import NucleusDB.Comms.Protocol.DIDCommSpec
import NucleusDB.Comms.Identity.SovereignBinding

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

open Comms.Identity

/-- Sender-side enrichment carried in authcrypt protected headers. -/
structure SenderEnrichmentSpec where
  senderDid : String
  evmAddress : Option String
  bindingProofSha256 : Option String
  deriving DecidableEq, Repr

/-- Enriched authcrypt witness extends base envelope semantics. -/
structure EnrichedAuthcryptEnvelopeSpec where
  base : AuthcryptEnvelopeSpec
  enrichment : Option SenderEnrichmentSpec
  deriving DecidableEq, Repr

/-- Well-formed enrichment: all optional fields are present together when provided. -/
def enrichmentWellFormed (enrichment : SenderEnrichmentSpec) : Prop :=
  enrichment.evmAddress.isSome ∧ enrichment.bindingProofSha256.isSome

/-- Enrichment binds sender when DID identities align. -/
def enrichmentBindsSender
    (env : EnrichedAuthcryptEnvelopeSpec) : Prop :=
  match env.enrichment with
  | none => True
  | some e =>
      enrichmentWellFormed e ∧ env.base.senderDid = e.senderDid

/-- Runtime-oriented acceptance predicate with enrichment side condition. -/
def acceptsEnrichedAuthcryptAt (now : Nat) (env : EnrichedAuthcryptEnvelopeSpec) : Prop :=
  acceptsAuthcryptAt now env.base ∧ enrichmentBindsSender env

theorem enrichment_binds_sender
    (env : EnrichedAuthcryptEnvelopeSpec)
    (h : acceptsEnrichedAuthcryptAt 0 env) :
    enrichmentBindsSender env := by
  exact h.2

theorem enrichment_round_trip
    (env : EnrichedAuthcryptEnvelopeSpec) :
    env = { base := env.base, enrichment := env.enrichment } := by
  rfl

end Protocol
end Comms
end NucleusDB
end HeytingLean
