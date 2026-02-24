// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "./TrustVerifier.sol";

/// @notice Interface for standard snarkjs-generated Groth16 verifiers.
/// @dev The verifier contract is generated from a circom circuit compiled with
///      6 public outputs matching the NucleusDB trust attestation convention:
///        [pufDigestLimb0, pufDigestLimb1, pufDigestLimb2, pufDigestLimb3, tier, replaySeq]
interface IGroth16Verifier {
    function verifyProof(
        uint256[2] calldata a,
        uint256[2][2] calldata b,
        uint256[2] calldata c,
        uint256[6] calldata pubSignals
    ) external view returns (bool);
}

/// @title Groth16VerifierAdapter
/// @notice Adapts a standard snarkjs-generated Groth16 verifier to the
///         ITrustProofVerifier interface used by TrustVerifier / TrustVerifierMultiChain.
///
/// @dev Proof encoding convention (256 bytes total):
///
///   proof = abi.encode(uint256[2] a, uint256[2][2] b, uint256[2] c)
///
///   Byte layout:
///     [  0: 32)  a[0]     — G1.x
///     [ 32: 64)  a[1]     — G1.y
///     [ 64: 96)  b[0][0]  — G2.x (imaginary component)
///     [ 96:128)  b[0][1]  — G2.x (real component)
///     [128:160)  b[1][0]  — G2.y (imaginary component)
///     [160:192)  b[1][1]  — G2.y (real component)
///     [192:224)  c[0]     — G1.x
///     [224:256)  c[1]     — G1.y
///
///   Public signals (6 × uint256):
///     [0..3] : PUF digest split into 4 × uint64 limbs (little-endian packing)
///     [4]    : tier enum value (1..4)
///     [5]    : monotone replay sequence number
///
///   Client-side encoding example (ethers.js):
///     const proof = ethers.AbiCoder.defaultAbiCoder().encode(
///       ["uint256[2]", "uint256[2][2]", "uint256[2]"],
///       [snarkProof.a, snarkProof.b, snarkProof.c]
///     );
///     const publicSignals = [limb0, limb1, limb2, limb3, tier, replaySeq];
///
contract Groth16VerifierAdapter is ITrustProofVerifier {
    IGroth16Verifier public immutable groth16;

    /// @notice Expected proof byte length: 8 BN254 field elements x 32 bytes.
    uint256 public constant PROOF_LENGTH = 256;

    /// @notice Required number of public signals (PUF digest limbs + tier + replaySeq).
    uint256 public constant SIGNAL_COUNT = 6;

    constructor(address groth16_) {
        groth16 = IGroth16Verifier(groth16_);
    }

    /// @inheritdoc ITrustProofVerifier
    function verifyProof(
        bytes calldata proof,
        uint256[] calldata publicSignals
    ) external view returns (bool) {
        if (proof.length != PROOF_LENGTH) return false;
        if (publicSignals.length != SIGNAL_COUNT) return false;

        // Decode ABI-encoded Groth16 proof points (a ∈ G1, b ∈ G2, c ∈ G1)
        (
            uint256[2] memory a,
            uint256[2][2] memory b,
            uint256[2] memory c
        ) = abi.decode(proof, (uint256[2], uint256[2][2], uint256[2]));

        // Copy dynamic-length signals into fixed-size array for the verifier
        uint256[6] memory sigs;
        sigs[0] = publicSignals[0];
        sigs[1] = publicSignals[1];
        sigs[2] = publicSignals[2];
        sigs[3] = publicSignals[3];
        sigs[4] = publicSignals[4];
        sigs[5] = publicSignals[5];

        return groth16.verifyProof(a, b, c, sigs);
    }
}
