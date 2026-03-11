namespace HeytingLean
namespace NucleusDB
namespace Crypto

/-- Authorization state for the PQ-gated EVM signing path. -/
structure AuthState where
  ed25519Authorized : Bool
  mldsa65Authorized : Bool
  deriving DecidableEq, Repr

/-- Initial state: no authorization is present. -/
def initialState : AuthState :=
  { ed25519Authorized := false, mldsa65Authorized := false }

/-- Runtime gate condition: both auth checks must hold. -/
def evmSignPermitted (s : AuthState) : Prop :=
  s.ed25519Authorized = true ∧ s.mldsa65Authorized = true

def authorizeEd25519 (s : AuthState) : AuthState :=
  { s with ed25519Authorized := true }

def authorizeMlDsa65 (s : AuthState) : AuthState :=
  { s with mldsa65Authorized := true }

def reset : AuthState := initialState

theorem evm_sign_requires_dual_auth
    (s : AuthState) :
    evmSignPermitted s → s.ed25519Authorized = true ∧ s.mldsa65Authorized = true :=
  fun h => h

theorem no_default_authorization :
    ¬ evmSignPermitted initialState := by
  intro h
  simp [initialState, evmSignPermitted] at h

theorem single_ed25519_auth_insufficient :
    ¬ evmSignPermitted (authorizeEd25519 initialState) := by
  intro h
  simp [authorizeEd25519, initialState, evmSignPermitted] at h

theorem single_mldsa65_auth_insufficient :
    ¬ evmSignPermitted (authorizeMlDsa65 initialState) := by
  intro h
  simp [authorizeMlDsa65, initialState, evmSignPermitted] at h

theorem authorization_composable :
    evmSignPermitted (authorizeMlDsa65 (authorizeEd25519 initialState)) := by
  simp [authorizeMlDsa65, authorizeEd25519, initialState, evmSignPermitted]

end Crypto
end NucleusDB
end HeytingLean
