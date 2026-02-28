import NucleusDB.Core.Authorization
import NucleusDB.Core.Certificates
import NucleusDB.Core.Ledger
import NucleusDB.Sheaf.MaterializationFunctor

namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Supported chains for the self-custodial WDK wallet. -/
inductive WalletChain where
  | bitcoin
  | ethereum
  | polygon
  | arbitrum
  deriving DecidableEq, Repr

/-- Wallet runtime state projected for formal reasoning. -/
structure WalletState where
  present : Bool
  unlocked : Bool
  bitcoinEnabled : Bool
  ethereumEnabled : Bool
  polygonEnabled : Bool
  arbitrumEnabled : Bool
  deriving DecidableEq, Repr

/-- Authorization witness for wallet transitions. -/
structure WalletAuth where
  actor : String
  authorized : Bool
  signatureBound : Bool
  requestDigestPresent : Bool
  deriving DecidableEq, Repr

/-- Wallet transition language mirrored by runtime wallet endpoints. -/
inductive WalletDelta where
  | create
  | importSeed
  | unlock
  | lock
  | delete
  deriving DecidableEq, Repr

/-- Deterministic wallet transition function. -/
def applyWalletDelta (s : WalletState) : WalletDelta → WalletState
  | .create =>
      { s with
        present := true
        unlocked := true
        bitcoinEnabled := true
        ethereumEnabled := true
        polygonEnabled := true
        arbitrumEnabled := true
      }
  | .importSeed =>
      { s with
        present := true
        unlocked := true
        bitcoinEnabled := true
        ethereumEnabled := true
        polygonEnabled := true
        arbitrumEnabled := true
      }
  | .unlock =>
      if s.present then { s with unlocked := true } else s
  | .lock =>
      if s.present then { s with unlocked := false } else s
  | .delete =>
      { present := false
        unlocked := false
        bitcoinEnabled := false
        ethereumEnabled := false
        polygonEnabled := false
        arbitrumEnabled := false }

/-- Delta-local policy obligations. -/
def walletDeltaAuthorized (s : WalletState) : WalletDelta → Prop
  | .create => s.present = false
  | .importSeed => s.present = false
  | .unlock => s.present = true ∧ s.unlocked = false
  | .lock => s.present = true ∧ s.unlocked = true
  | .delete => s.present = true

/-- Wallet policy used by certificate and ledger verification. -/
def walletPolicy :
    Core.AuthorizationPolicy WalletState WalletDelta WalletAuth :=
  fun s d auth =>
    auth.authorized = true
      ∧ auth.actor.length > 0
      ∧ auth.signatureBound = true
      ∧ auth.requestDigestPresent = true
      ∧ walletDeltaAuthorized s d

/-- Wallet-specialized certificate and ledger aliases. -/
abbrev WalletCommitCertificate :=
  Core.CommitCertificate WalletState WalletDelta WalletAuth applyWalletDelta walletPolicy

abbrev WalletCommitRecord :=
  Core.CommitRecord WalletState WalletDelta WalletAuth applyWalletDelta walletPolicy

abbrev verifyWalletLedger :=
  Core.verifyLedger
    (State := WalletState)
    (Delta := WalletDelta)
    (Auth := WalletAuth)
    (apply := applyWalletDelta)
    (policy := walletPolicy)

/-- POD-facing wallet projection keys. -/
inductive WalletPodKey where
  | walletPresent
  | walletUnlocked
  | bitcoinEnabled
  | ethereumEnabled
  | polygonEnabled
  | arbitrumEnabled
  deriving DecidableEq, Repr

def materializeWallet (s : WalletState) : WalletPodKey → String
  | .walletPresent => toString s.present
  | .walletUnlocked => toString s.unlocked
  | .bitcoinEnabled => toString s.bitcoinEnabled
  | .ethereumEnabled => toString s.ethereumEnabled
  | .polygonEnabled => toString s.polygonEnabled
  | .arbitrumEnabled => toString s.arbitrumEnabled

/-- Transport relation preserving wallet POD-visible projection. -/
def walletTransports (s t : WalletState) : Prop :=
  s.present = t.present
    ∧ s.unlocked = t.unlocked
    ∧ s.bitcoinEnabled = t.bitcoinEnabled
    ∧ s.ethereumEnabled = t.ethereumEnabled
    ∧ s.polygonEnabled = t.polygonEnabled
    ∧ s.arbitrumEnabled = t.arbitrumEnabled

/-- Wallet state materialization as a sheaf-compatible functor. -/
def walletMaterializationFunctor :
    Sheaf.MaterializationFunctor WalletState WalletPodKey String where
  toVector := materializeWallet
  transports := walletTransports
  naturality := by
    intro s t h
    rcases h with ⟨hPresent, hUnlocked, hBtc, hEth, hPoly, hArb⟩
    funext k
    cases k <;>
      simp [materializeWallet, hPresent, hUnlocked, hBtc, hEth, hPoly, hArb]

theorem walletPolicy_rejects_unauthorized
    (s : WalletState) (d : WalletDelta) (actor : String) :
    ¬ walletPolicy s d
      { actor := actor
        authorized := false
        signatureBound := true
        requestDigestPresent := true } := by
  simp [walletPolicy]

theorem walletApplyDelete_clears_state (s : WalletState) :
    applyWalletDelta s .delete =
      { present := false
        unlocked := false
        bitcoinEnabled := false
        ethereumEnabled := false
        polygonEnabled := false
        arbitrumEnabled := false } := by
  rfl

theorem verifyWalletLedger_nil : verifyWalletLedger [] := by
  exact Core.verifyLedger_nil

end Identity
end NucleusDB
end HeytingLean
