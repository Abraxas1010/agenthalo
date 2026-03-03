import NucleusDB.Comms.DIDComm.EnvelopeSpec

/-!
# Mesh Authorization Chain Formal Specification

T24: An authorized mesh request has a verified sender identity (envelope authentic).
T25: An authorized mesh request has an unexpired capability grant.
Additional: wildcard grants cover all tools, broken chains reject.
-/

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Mesh

open DIDComm

/-- A mesh request bundles an authenticated envelope with a tool call. -/
structure MeshRequest where
  envelope : Envelope
  toolName : String
  senderDid : String
  deriving Repr

/-- A capability grant authorizes a specific tool call. -/
structure CapabilityGrant where
  granteeDid : String
  toolPattern : String
  expiresAt : Nat
  deriving Repr

/-- Tool name matches a capability pattern (exact match or wildcard "*"). -/
def toolMatchesGrant (grant : CapabilityGrant) (toolName : String) : Bool :=
  grant.toolPattern == "*" || grant.toolPattern == toolName

/-- Request is authorized if:
    1. Envelope is authentic (dual signature verifies)
    2. There exists a valid, unexpired capability grant
    3. The grant covers the requested tool -/
def requestAuthorized
    (req : MeshRequest)
    (edPk pqPk : List Nat)
    (grants : List CapabilityGrant)
    (now : Nat) : Prop :=
  envelopeAuthentic req.envelope edPk pqPk
  ∧ ∃ g ∈ grants,
      g.granteeDid = req.senderDid
      ∧ now < g.expiresAt
      ∧ toolMatchesGrant g req.toolName = true

/-- T24: An authorized mesh request has a verified sender identity. -/
theorem authorized_implies_authentic
    (req : MeshRequest) (edPk pqPk : List Nat)
    (grants : List CapabilityGrant) (now : Nat)
    (h : requestAuthorized req edPk pqPk grants now) :
    envelopeAuthentic req.envelope edPk pqPk :=
  h.1

/-- T25: An authorized mesh request has an unexpired capability. -/
theorem authorized_implies_unexpired
    (req : MeshRequest) (edPk pqPk : List Nat)
    (grants : List CapabilityGrant) (now : Nat)
    (h : requestAuthorized req edPk pqPk grants now) :
    ∃ g ∈ grants, now < g.expiresAt := by
  obtain ⟨_, g, hg_mem, _, hg_exp, _⟩ := h
  exact ⟨g, hg_mem, hg_exp⟩

/-- An authorized request has a matching capability grant. -/
theorem authorized_implies_tool_match
    (req : MeshRequest) (edPk pqPk : List Nat)
    (grants : List CapabilityGrant) (now : Nat)
    (h : requestAuthorized req edPk pqPk grants now) :
    ∃ g ∈ grants, toolMatchesGrant g req.toolName = true := by
  obtain ⟨_, g, hg_mem, _, _, hg_match⟩ := h
  exact ⟨g, hg_mem, hg_match⟩

/-- Without envelope authentication, request cannot be authorized. -/
theorem unauthentic_request_rejected
    (req : MeshRequest) (edPk pqPk : List Nat)
    (grants : List CapabilityGrant) (now : Nat)
    (hNotAuth : ¬ envelopeAuthentic req.envelope edPk pqPk) :
    ¬ requestAuthorized req edPk pqPk grants now := by
  intro h
  exact hNotAuth h.1

/-- With no grants, request cannot be authorized. -/
theorem no_grants_rejects
    (req : MeshRequest) (edPk pqPk : List Nat) (now : Nat) :
    ¬ requestAuthorized req edPk pqPk [] now := by
  intro ⟨_, g, hg_mem, _⟩
  exact nomatch hg_mem

/-- A wildcard grant ("*") matches any tool name. -/
theorem wildcard_grant_matches_any (toolName : String) :
    toolMatchesGrant { granteeDid := "did:key:z6Mk...", toolPattern := "*", expiresAt := 0 }
      toolName = true := by
  simp [toolMatchesGrant]

/-- An exact grant matches only its specific tool. -/
theorem exact_grant_matches_self :
    toolMatchesGrant
      { granteeDid := "did:key:z6Mk...", toolPattern := "nucleusdb_query", expiresAt := 0 }
      "nucleusdb_query" = true := by
  native_decide

/-- An exact grant does not match a different tool. -/
theorem exact_grant_rejects_other :
    toolMatchesGrant
      { granteeDid := "did:key:z6Mk...", toolPattern := "nucleusdb_query", expiresAt := 0 }
      "nucleusdb_execute_sql" = false := by
  native_decide

/-- All expired grants cannot authorize (regardless of tool match). -/
theorem all_expired_rejects
    (req : MeshRequest) (edPk pqPk : List Nat)
    (grants : List CapabilityGrant) (now : Nat)
    (hAllExpired : ∀ g ∈ grants, now ≥ g.expiresAt) :
    ¬ requestAuthorized req edPk pqPk grants now := by
  intro ⟨_, g, hg_mem, _, hg_exp, _⟩
  exact Nat.not_lt_of_ge (hAllExpired g hg_mem) hg_exp

/-- Authorization is constructible from its components. -/
theorem authorize_from_components
    (req : MeshRequest) (edPk pqPk : List Nat)
    (g : CapabilityGrant) (grants : List CapabilityGrant) (now : Nat)
    (hAuth : envelopeAuthentic req.envelope edPk pqPk)
    (hMem : g ∈ grants)
    (hDid : g.granteeDid = req.senderDid)
    (hExp : now < g.expiresAt)
    (hMatch : toolMatchesGrant g req.toolName = true) :
    requestAuthorized req edPk pqPk grants now :=
  ⟨hAuth, g, hMem, hDid, hExp, hMatch⟩

end Mesh
end Comms
end NucleusDB
end HeytingLean
