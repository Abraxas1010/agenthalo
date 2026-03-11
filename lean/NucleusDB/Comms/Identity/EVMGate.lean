namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Identity

structure AuthState where
  ed25519Authorized : Bool
  mldsa65Authorized : Bool
  deriving DecidableEq, Repr

def initialState : AuthState :=
  { ed25519Authorized := false, mldsa65Authorized := false }

def evmSignPermitted (s : AuthState) : Prop :=
  s.ed25519Authorized = true ∧ s.mldsa65Authorized = true

def authorizeEd25519 (s : AuthState) : AuthState :=
  { s with ed25519Authorized := true }

def authorizeMlDsa65 (s : AuthState) : AuthState :=
  { s with mldsa65Authorized := true }

/-- T39: EVM signing requires both DID authorization checks. -/
theorem evm_sign_requires_dual_auth
    (s : AuthState) :
    evmSignPermitted s → s.ed25519Authorized = true ∧ s.mldsa65Authorized = true :=
  fun h => h

/-- T40: signing is not authorized by default. -/
theorem no_default_authorization :
    ¬ evmSignPermitted initialState := by
  intro h
  simp [initialState, evmSignPermitted] at h

/-- T41: single-factor authorization is insufficient. -/
theorem single_auth_insufficient :
    ¬ evmSignPermitted (authorizeEd25519 initialState) ∧
      ¬ evmSignPermitted (authorizeMlDsa65 initialState) := by
  constructor <;> intro h <;> simp [authorizeEd25519, authorizeMlDsa65, initialState, evmSignPermitted] at h

/-- T42: the two authorization steps compose to permit signing. -/
theorem authorization_composable :
    evmSignPermitted (authorizeMlDsa65 (authorizeEd25519 initialState)) := by
  simp [authorizeMlDsa65, authorizeEd25519, initialState, evmSignPermitted]

end Identity
end Comms
end NucleusDB
end HeytingLean
