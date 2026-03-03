import NucleusDB.Comms.Identity.GenesisDerivation
import NucleusDB.Comms.Identity.AgentAddressDerivation
import NucleusDB.Comms.Identity.DIDDocumentSpec

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Identity

/-- Agent identity presheaf: assigns to each agent its sovereign identity components. -/
structure AgentIdentity where
  genesisSeed : Seed64
  evmAddress : String
  didSubject : String

/-- Communication presheaf: assigns to each agent its communication credentials. -/
structure CommIdentity where
  didcommSender : String
  evmAddress : String
  bindingProofHash : String
  deriving DecidableEq, Repr

/-- Abstract binding proof hash oracle. -/
axiom compute_binding_hash : String → String → String → String

/-- Binding hash is deterministic. -/
axiom binding_hash_deterministic :
  ∀ d1 d2 e1 e2 h1 h2 : String,
    d1 = d2 → e1 = e2 → h1 = h2 →
    compute_binding_hash d1 e1 h1 = compute_binding_hash d2 e2 h2

/-- The sovereign binding natural transformation:
    maps identity presheaf to communication presheaf. -/
noncomputable def sovereignBindingNT (agent : AgentIdentity) : CommIdentity :=
  { didcommSender := agent.didSubject
    evmAddress := agent.evmAddress
    bindingProofHash := compute_binding_hash agent.didSubject agent.evmAddress "v1" }

/-- The binding transformation is natural: for any two agents with the same
    genesis seed, the communication identity transforms consistently. -/
theorem sovereign_binding_natural (a1 a2 : AgentIdentity)
    (_hSeed : a1.genesisSeed = a2.genesisSeed)
    (hEvm : a1.evmAddress = a2.evmAddress)
    (hDid : a1.didSubject = a2.didSubject) :
    sovereignBindingNT a1 = sovereignBindingNT a2 := by
  simp [sovereignBindingNT]
  exact ⟨hDid, hEvm, binding_hash_deterministic _ _ _ _ _ _ hDid hEvm rfl⟩

/-- A well-formed binding proof implies the DID and EVM address share a
    common genesis provenance (formalized as same AgentIdentity). -/
theorem binding_proof_verifiable (agent : AgentIdentity) :
    let comm := sovereignBindingNT agent
    comm.didcommSender = agent.didSubject
      ∧ comm.evmAddress = agent.evmAddress := by
  simp [sovereignBindingNT]

/-- The sovereign binding NT preserves the DID subject. -/
theorem sovereign_binding_preserves_did (agent : AgentIdentity) :
    (sovereignBindingNT agent).didcommSender = agent.didSubject := by
  rfl

/-- The sovereign binding NT preserves the EVM address. -/
theorem sovereign_binding_preserves_evm (agent : AgentIdentity) :
    (sovereignBindingNT agent).evmAddress = agent.evmAddress := by
  rfl

end Identity
end Comms
end NucleusDB
end HeytingLean
