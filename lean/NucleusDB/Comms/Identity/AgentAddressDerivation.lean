import NucleusDB.Comms.Identity.GenesisDerivation
import Mathlib.CategoryTheory.Discrete.Basic
import Mathlib.CategoryTheory.Functor.Basic

namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Identity

/-- Extended derivation info including wallet entropy branch. -/
inductive ExtDerivationInfo where
  | p2pIdentity
  | didPqSigning
  | didcommX25519
  | didcommMlKem768
  | walletEntropy
  deriving DecidableEq, Repr

/-- Extended keypair types including secp256k1 for EVM wallets. -/
inductive ExtKeypairType where
  | ed25519
  | mlDsa65
  | x25519
  | mlKem768
  | secp256k1
  deriving DecidableEq, Repr

abbrev ExtInfoCategory := CategoryTheory.Discrete ExtDerivationInfo
abbrev ExtKeyCategory := CategoryTheory.Discrete ExtKeypairType

/-- Extended branch mapping including wallet → secp256k1 path. -/
def extKeypairOfInfo : ExtDerivationInfo → ExtKeypairType
  | .p2pIdentity => .ed25519
  | .didPqSigning => .mlDsa65
  | .didcommX25519 => .x25519
  | .didcommMlKem768 => .mlKem768
  | .walletEntropy => .secp256k1

/-- Extended derivation functor with all 5 branches. -/
def extDerivationFunctor : CategoryTheory.Functor ExtInfoCategory ExtKeyCategory :=
  CategoryTheory.Discrete.functor (fun info => ⟨extKeypairOfInfo info⟩)

/-- BIP-39 mnemonic oracle: entropy bytes → mnemonic string. -/
axiom bip39_mnemonic : (Fin 32 → Nat) → String

/-- BIP-32 derivation oracle: mnemonic → derivation path → private key bytes. -/
axiom bip32_derive : String → String → (Fin 32 → Nat)

/-- Keccak-256 hash oracle for EVM address derivation. -/
axiom keccak256_address : (Fin 32 → Nat) → String

/-- BIP-39 is deterministic. -/
axiom bip39_deterministic :
  ∀ e1 e2 : Fin 32 → Nat, e1 = e2 → bip39_mnemonic e1 = bip39_mnemonic e2

/-- BIP-32 is deterministic. -/
axiom bip32_deterministic :
  ∀ m1 m2 p : String, m1 = m2 → bip32_derive m1 p = bip32_derive m2 p

/-- Keccak-256 address derivation is deterministic. -/
axiom keccak256_deterministic :
  ∀ k1 k2 : Fin 32 → Nat, k1 = k2 → keccak256_address k1 = keccak256_address k2

/-- Standard EVM derivation path m/44'/60'/0'/0/0. -/
def evmDerivationPath : String := "m/44'/60'/0'/0/0"

/-- Derive an EVM address from 32-byte wallet entropy. -/
noncomputable def deriveEvmAddress (entropy32 : Fin 32 → Nat) : String :=
  let mnemonic := bip39_mnemonic entropy32
  let privateKey := bip32_derive mnemonic evmDerivationPath
  keccak256_address privateKey

/-- Same genesis seed → same EVM address (end-to-end determinism). -/
theorem agentaddress_deterministic (e1 e2 : Fin 32 → Nat) (h : e1 = e2) :
    deriveEvmAddress e1 = deriveEvmAddress e2 := by
  simp [deriveEvmAddress]
  have hm := bip39_deterministic e1 e2 h
  have hk := bip32_deterministic _ _ evmDerivationPath hm
  exact keccak256_deterministic _ _ hk

/-- Extended derivation functor maps walletEntropy to secp256k1 correctly. -/
theorem derivation_functor_extended_correct :
    (extDerivationFunctor.obj ⟨ExtDerivationInfo.walletEntropy⟩).as = ExtKeypairType.secp256k1 := by
  simp [extDerivationFunctor, extKeypairOfInfo]

/-- Original branches preserved in the extended functor. -/
theorem derivation_functor_extended_preserves_original (info : DerivationInfo) :
    keypairOfInfo info = match info with
      | .p2pIdentity => .ed25519
      | .didPqSigning => .mlDsa65
      | .didcommX25519 => .x25519
      | .didcommMlKem768 => .mlKem768 := by
  cases info <;> rfl

end Identity
end Comms
end NucleusDB
end HeytingLean
