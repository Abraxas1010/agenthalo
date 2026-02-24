# NucleusDB Trust Attestation Circuit

Groth16 circuit for proving PUF identity ownership without revealing the raw PUF response.

## Public Signal Convention (6 signals)

| Index | Name | Description |
|-------|------|-------------|
| 0 | `pufDigestLimb0` | SHA-256(pufResponse) bits [0:64), little-endian |
| 1 | `pufDigestLimb1` | SHA-256(pufResponse) bits [64:128) |
| 2 | `pufDigestLimb2` | SHA-256(pufResponse) bits [128:192) |
| 3 | `pufDigestLimb3` | SHA-256(pufResponse) bits [192:256) |
| 4 | `tier` | Agent tier enum (1..4) |
| 5 | `replaySeq` | Monotone replay sequence (> 0) |

## Circuit Security Properties

1. The raw PUF response (256-bit private witness) is never revealed.
2. The prover demonstrates knowledge of a SHA-256 preimage matching the public digest.
3. Tier is constrained to `[1, 4]` (mirrors Solidity `InvalidTier` revert).
4. Replay sequence is constrained to `> 0`.

Approximate constraint count: ~29,000 (dominated by SHA-256).

## Prerequisites

```bash
# Install circom (v2.1.0+)
curl -sSL https://raw.githubusercontent.com/nicmcd/circom-installer/main/install.sh | bash

# Install snarkjs
npm install -g snarkjs

# Install circomlib (SHA-256, comparators, bitify)
mkdir -p node_modules
npm install circomlib
```

## Compilation and Setup

```bash
cd contracts/circuits

# 1. Compile circuit
circom trust_attestation.circom --r1cs --wasm --sym -o build/

# 2. Download Powers of Tau (BN254, sufficient for ~29k constraints)
#    For production, use a ceremony-verified ptau file.
snarkjs powersoftau new bn128 16 pot16_0000.ptau -v
snarkjs powersoftau contribute pot16_0000.ptau pot16_0001.ptau \
  --name="NucleusDB Phase 1" -v
snarkjs powersoftau prepare phase2 pot16_0001.ptau pot16_final.ptau -v

# 3. Circuit-specific setup
snarkjs groth16 setup build/trust_attestation.r1cs pot16_final.ptau \
  trust_attestation_0000.zkey
snarkjs zkey contribute trust_attestation_0000.zkey trust_attestation_final.zkey \
  --name="NucleusDB Phase 2" -v

# 4. Export verification key
snarkjs zkey export verificationkey trust_attestation_final.zkey verification_key.json

# 5. Generate Solidity verifier
snarkjs zkey export solidityverifier trust_attestation_final.zkey \
  ../Groth16TrustVerifier.sol
```

The generated `Groth16TrustVerifier.sol` will have the standard interface:

```solidity
function verifyProof(
    uint256[2] calldata a,
    uint256[2][2] calldata b,
    uint256[2] calldata c,
    uint256[6] calldata pubSignals
) public view returns (bool)
```

This matches `IGroth16Verifier` in `Groth16VerifierAdapter.sol`.

## Generating a Proof

```bash
# 1. Create input.json with witness values
cat > input.json << 'EOF'
{
  "pufResponse": [1,0,1,...],  // 256 bits from PUF challenge-response
  "tierIn": 2,                 // Agent tier
  "replaySeqIn": 42            // Monotone sequence number
}
EOF

# 2. Generate witness
node build/trust_attestation_js/generate_witness.js \
  build/trust_attestation_js/trust_attestation.wasm \
  input.json witness.wtns

# 3. Generate Groth16 proof
snarkjs groth16 prove trust_attestation_final.zkey witness.wtns \
  proof.json public.json

# 4. Verify locally
snarkjs groth16 verify verification_key.json public.json proof.json
```

## On-Chain Deployment

Deploy in this order:

```bash
# 1. Deploy the snarkjs-generated verifier
forge create Groth16TrustVerifier --rpc-url $RPC_URL --private-key $PK

# 2. Deploy the adapter (wraps snarkjs verifier into ITrustProofVerifier)
forge create Groth16VerifierAdapter --constructor-args $GROTH16_ADDRESS \
  --rpc-url $RPC_URL --private-key $PK

# 3. Deploy TrustVerifierMultiChain with the adapter as verifier
forge create TrustVerifierMultiChain \
  --constructor-args $ADAPTER_ADDRESS $USDC_ADDRESS $TREASURY $FEE $DEFAULT_FEE \
  --rpc-url $RPC_URL --private-key $PK
```

## Client-Side Proof Encoding

The `Groth16VerifierAdapter` expects the proof as ABI-encoded bytes:

```javascript
const { AbiCoder } = require("ethers");

// snarkjs proof output: { pi_a, pi_b, pi_c }
const proof = AbiCoder.defaultAbiCoder().encode(
  ["uint256[2]", "uint256[2][2]", "uint256[2]"],
  [
    [snarkProof.pi_a[0], snarkProof.pi_a[1]],
    [
      [snarkProof.pi_b[0][0], snarkProof.pi_b[0][1]],
      [snarkProof.pi_b[1][0], snarkProof.pi_b[1][1]]
    ],
    [snarkProof.pi_c[0], snarkProof.pi_c[1]]
  ]
);

const publicSignals = [limb0, limb1, limb2, limb3, tier, replaySeq];
```

## Trusted Setup Notes

The Powers of Tau ceremony above is for development only. For production:

1. Use a publicly verifiable ceremony (e.g., Hermez, Zcash, or a dedicated NucleusDB ceremony).
2. The circuit-specific Phase 2 contribution should involve multiple independent parties.
3. Publish the final `.ptau` and `.zkey` files alongside their contribution hashes.
4. The verification key hash should be committed on-chain or in the NucleusDB seal chain.
