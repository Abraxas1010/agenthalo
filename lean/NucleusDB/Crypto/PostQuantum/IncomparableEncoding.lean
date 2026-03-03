/-!
# Crypto.PostQuantum.IncomparableEncoding

Abstract interface for *incomparable encodings* as used by the `leanEthereum` LeanSig / generalized
XMSS construction.

Design goal: stay executable-first and policy-compliant:
- no proof-hole shortcuts;
- no cryptographic postulates;
- security claims are expressed as named `Prop` interfaces, mirroring
  `HeytingLean.Crypto.Signature.Spec`.

Upstream reference (Rust):
- `leanEthereum/leanSig`, `src/inc_encoding.rs` and `src/inc_encoding/target_sum.rs`

Papers:
- "Hash-Based Multi-Signatures for Post-Quantum Ethereum" (ePrint 2025/055)
  https://eprint.iacr.org/2025/055.pdf
- "LeanSig for Post-Quantum Ethereum" (ePrint 2025/1332)
  https://eprint.iacr.org/2025/1332.pdf
-/

namespace NucleusDB
namespace Crypto
namespace PostQuantum
namespace IncomparableEncoding

/-- An incomparable-encoding scheme interface.

`encode` may fail (`none`), matching the “retry with fresh randomness” usage pattern in LeanSig.
`decode` is included as an optional spec-level companion (some constructions do not expose a
useful decoder). -/
structure Scheme where
  Msg   : Type
  Epoch : Type
  Rand  : Type
  Code  : Type
  Param : Type
  encode : Param → Epoch → Rand → Msg → Option Code
  decode : Param → Epoch → Code → Option Msg

namespace Scheme

/-- Correctness: any successful encoding decodes back to the original message. -/
def Correct (S : Scheme) : Prop :=
  ∀ (pp : S.Param) (e : S.Epoch) (r : S.Rand) (m : S.Msg) (c : S.Code),
    S.encode pp e r m = some c → S.decode pp e c = some m

/-- A (very strong, information-theoretic) “no collisions” property:
distinct messages cannot encode (in the same epoch) to the same codeword. -/
def Incomparable (S : Scheme) : Prop :=
  ∀ (pp : S.Param) (e : S.Epoch) (r₁ r₂ : S.Rand) (m₁ m₂ : S.Msg) (c : S.Code),
    m₁ ≠ m₂ →
    S.encode pp e r₁ m₁ = some c →
    S.encode pp e r₂ m₂ ≠ some c

/-- Security properties (e.g. target collision resistance).

This is intentionally abstract, mirroring `Crypto.Signature.Spec.Scheme.SecurityProps`. -/
structure SecurityProps (S : Scheme) where
  tcr : Prop

end Scheme

end IncomparableEncoding
end PostQuantum
end Crypto
end NucleusDB
