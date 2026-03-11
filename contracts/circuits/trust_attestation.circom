pragma circom 2.1.0;

include "circomlib/circuits/sha256/sha256.circom";
include "circomlib/circuits/comparators.circom";
include "circomlib/circuits/bitify.circom";

/// @title TrustAttestation
/// @notice Proves PUF identity ownership without revealing the raw PUF response.
///
/// @dev Public signal convention (6 outputs, matching Solidity contract expectations):
///   signal output pufDigestLimb0  — SHA-256(pufResponse) bits [0:64)
///   signal output pufDigestLimb1  — SHA-256(pufResponse) bits [64:128)
///   signal output pufDigestLimb2  — SHA-256(pufResponse) bits [128:192)
///   signal output pufDigestLimb3  — SHA-256(pufResponse) bits [192:256)
///   signal output tier            — Agent tier (1..4)
///   signal output replaySeq       — Monotone replay sequence number (> 0)
///
/// @dev Private witness:
///   signal input pufResponse[256]  — Raw PUF challenge-response bits (256 bits)
///   signal input tierIn            — Tier value (passed through to public output)
///   signal input replaySeqIn       — Replay sequence (passed through to public output)
///
/// @dev Security properties:
///   1. The PUF response is never revealed — only its SHA-256 digest is public.
///   2. The prover demonstrates knowledge of a preimage that hashes to the
///      digest encoded in the public signals.
///   3. Tier is constrained to [1, 4] (matching Solidity's InvalidTier check).
///   4. Replay sequence is constrained to be positive (> 0).
///
/// @dev Approximate circuit size: ~29,000 constraints (dominated by SHA-256).
template TrustAttestation() {
    // ── Private inputs (witness) ──
    signal input pufResponse[256];
    signal input tierIn;
    signal input replaySeqIn;

    // ── Public outputs (exposed as publicSignals to verifier) ──
    signal output pufDigestLimb0;
    signal output pufDigestLimb1;
    signal output pufDigestLimb2;
    signal output pufDigestLimb3;
    signal output tier;
    signal output replaySeq;

    // ── Step 1: Constrain PUF response bits to {0, 1} ──
    for (var i = 0; i < 256; i++) {
        pufResponse[i] * (1 - pufResponse[i]) === 0;
    }

    // ── Step 2: Compute SHA-256 of PUF response ──
    component hasher = Sha256(256);
    for (var i = 0; i < 256; i++) {
        hasher.in[i] <== pufResponse[i];
    }

    // ── Step 3: Pack hash output bits into 4 × 64-bit limbs ──
    // Little-endian packing: limb0 = bits [0:64), limb1 = bits [64:128), etc.
    // This matches the Solidity _decodeDigest() convention in TrustVerifier.sol.
    component pack0 = Bits2Num(64);
    component pack1 = Bits2Num(64);
    component pack2 = Bits2Num(64);
    component pack3 = Bits2Num(64);

    for (var i = 0; i < 64; i++) {
        pack0.in[i] <== hasher.out[i];
        pack1.in[i] <== hasher.out[64 + i];
        pack2.in[i] <== hasher.out[128 + i];
        pack3.in[i] <== hasher.out[192 + i];
    }

    pufDigestLimb0 <== pack0.out;
    pufDigestLimb1 <== pack1.out;
    pufDigestLimb2 <== pack2.out;
    pufDigestLimb3 <== pack3.out;

    // ── Step 4: Constrain tier to [1, 4] ──
    component tierGe = GreaterEqThan(8);
    tierGe.in[0] <== tierIn;
    tierGe.in[1] <== 1;
    tierGe.out === 1;

    component tierLe = LessEqThan(8);
    tierLe.in[0] <== tierIn;
    tierLe.in[1] <== 4;
    tierLe.out === 1;

    tier <== tierIn;

    // ── Step 5: Constrain replay sequence > 0 ──
    component seqGt = GreaterThan(64);
    seqGt.in[0] <== replaySeqIn;
    seqGt.in[1] <== 0;
    seqGt.out === 1;

    replaySeq <== replaySeqIn;
}

component main = TrustAttestation();
