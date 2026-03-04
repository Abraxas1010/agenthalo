# PQ Assessment: EVM Wallet

**Date:** 2026-03-04
**Scope:** WP-6 — Post-quantum readiness of EVM wallet integration

## Current Architecture

`src/halo/evm_wallet.rs` derives an Ethereum-compatible wallet from the agent's
BIP-39 mnemonic via BIP-32 hierarchical derivation:

- **Curve:** secp256k1 (ECDSA)
- **Derivation:** `m/44'/60'/0'/0/0` (standard Ethereum path)
- **Address:** Keccak-256 hash of uncompressed public key (last 20 bytes)
- **Signing:** ECDSA over secp256k1 with recoverable signatures

## PQ Exposure Analysis

### ECDSA Signatures (secp256k1)

Shor's algorithm can recover secp256k1 private keys from public keys in
polynomial time on a fault-tolerant quantum computer. Once an agent's EVM
address has been used in a transaction (exposing the public key), the private
key is recoverable.

**Impact:** An attacker with a quantum computer can:
1. Forge EVM transactions from any agent whose public key is known
2. Drain EVM wallet funds
3. Impersonate the agent on-chain

### BIP-32 Derivation

BIP-32 uses HMAC-SHA512 for child key derivation. SHA-512 is quantum-resistant
(Grover's gives at most quadratic speedup, still 256-bit security). The
derivation chain itself is not vulnerable.

### Keccak-256 Address Derivation

Keccak-256 is quantum-resistant (128-bit collision resistance under Grover's).
Address derivation is not vulnerable.

## Threat Assessment

| Component | Quantum Vulnerable | Impact if Broken | Urgency |
|-----------|-------------------|------------------|---------|
| secp256k1 ECDSA signing | YES | Fund theft, impersonation | LOW* |
| BIP-32 derivation (HMAC-SHA512) | NO | N/A | NONE |
| Keccak-256 address | NO | N/A | NONE |
| On-chain transaction history | YES (retroactive) | Historical key recovery | LOW* |

*LOW because: (a) the entire Ethereum ecosystem shares this vulnerability,
(b) Ethereum's own PQ migration must happen first (EIP-7702 and related proposals),
(c) AgentHALO cannot unilaterally adopt PQ EVM signatures without EVM support.

## Mitigation Status

- **AgentHALO cannot fix this unilaterally.** EVM transaction signature schemes
  are defined by the Ethereum protocol. Until Ethereum supports PQ signature
  algorithms (e.g., via account abstraction with PQ signers), all EVM wallets
  share the same vulnerability.
- **BIP-32 seed derivation is already SHA-512** — no upgrade needed.
- **DIDComm identity is PQ-hardened** (hybrid KEM + ML-DSA-65 signatures).
  The EVM wallet is an auxiliary feature, not the primary identity system.

## Recommendations

1. **No code changes required now.** The EVM wallet is an ecosystem-shared
   vulnerability with no unilateral fix available.
2. **Monitor Ethereum PQ proposals:**
   - EIP-7702 (account abstraction — enables custom signature verification)
   - Vitalik's "quantum emergency" plan (hard fork to lock exposed accounts)
   - Lattice-based signature schemes for EVM (research stage)
3. **When Ethereum supports PQ signatures:** Update `evm_wallet.rs` to derive
   a PQ signing key (e.g., ML-DSA-65 or SLH-DSA) alongside secp256k1, and
   use account abstraction to accept PQ signatures on-chain.
4. **Defense-in-depth (optional):** For high-value on-chain operations, consider
   a multisig scheme where at least one signer is a PQ-safe off-chain
   verification (e.g., DIDComm-verified approval before EVM transaction broadcast).

## Conclusion

The EVM wallet's secp256k1 ECDSA is quantum-vulnerable, but this is an
ecosystem-wide issue shared by all Ethereum users. AgentHALO's primary identity
and communication layers are PQ-hardened. The EVM wallet is an auxiliary
integration that will inherit PQ protection when Ethereum itself migrates.
No code changes are actionable today.
