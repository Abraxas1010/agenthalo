// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

/// @notice Minimal ERC-20 interface for USDC-compatible payment routing.
interface IERC20 {
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

/// @notice External Groth16 verifier interface generated out-of-band.
interface ITrustProofVerifier {
    function verifyProof(
        bytes calldata proof,
        uint256[] calldata publicSignals
    ) external view returns (bool);
}

/// @title NucleusDB TrustVerifier
/// @notice On-chain trust attestation + payment routing for agent certificates.
contract TrustVerifier {
    error InvalidProof();
    error TransferFailed();
    error InvalidDigest();
    error InvalidTier();
    error SequenceRegression();

    event AgentCertified(
        address indexed agent,
        bytes32 indexed pufDigest,
        uint8 tier,
        uint64 indexed timestamp,
        uint64 replaySeq,
        uint256 feePaid
    );

    struct AgentRecord {
        bytes32 pufDigest;
        uint64 lastAttestation;
        uint64 lastReplaySeq;
        uint8 tier;
        bool active;
    }

    IERC20 public immutable usdc;
    ITrustProofVerifier public immutable verifier;
    address public immutable treasury;
    uint256 public immutable feeWei;

    mapping(address => AgentRecord) public registry;

    constructor(
        address verifier_,
        address usdc_,
        address treasury_,
        uint256 feeWei_
    ) {
        verifier = ITrustProofVerifier(verifier_);
        usdc = IERC20(usdc_);
        treasury = treasury_;
        feeWei = feeWei_;
    }

    /// @notice Verify trust proof, register/refresh agent attestation, and route payment.
    /// @dev publicSignals convention:
    ///  - [0..3] : puf digest split into 4 x uint64 limbs (little-endian packing)
    ///  - [4]    : tier enum value (1..4)
    ///  - [5]    : monotone sequence / replay bound
    function attestAndPay(
        bytes calldata proof,
        uint256[] calldata publicSignals
    ) external {
        if (publicSignals.length < 6) revert InvalidDigest();
        if (!verifier.verifyProof(proof, publicSignals)) revert InvalidProof();

        bytes32 pufDigest = _decodeDigest(publicSignals);
        if (pufDigest == bytes32(0)) revert InvalidDigest();
        uint8 tier = uint8(publicSignals[4]);
        if (tier == 0 || tier > 4) revert InvalidTier();
        uint64 replaySeq = uint64(publicSignals[5]);

        AgentRecord memory prev = registry[msg.sender];
        if (prev.active && replaySeq <= prev.lastReplaySeq) revert SequenceRegression();

        registry[msg.sender] = AgentRecord({
            pufDigest: pufDigest,
            lastAttestation: uint64(block.timestamp),
            lastReplaySeq: replaySeq,
            tier: tier,
            active: true
        });

        if (feeWei > 0) {
            bool ok = usdc.transferFrom(msg.sender, treasury, feeWei);
            if (!ok) revert TransferFailed();
        }

        emit AgentCertified(msg.sender, pufDigest, tier, uint64(block.timestamp), replaySeq, feeWei);
    }

    function verifyAgent(address agent) external view returns (bool) {
        return registry[agent].active;
    }

    function agentStatus(
        address agent
    ) external view returns (bool active, bytes32 pufDigest, uint8 tier, uint64 lastAttestation, uint64 lastReplaySeq) {
        AgentRecord memory record = registry[agent];
        return (record.active, record.pufDigest, record.tier, record.lastAttestation, record.lastReplaySeq);
    }

    function _decodeDigest(uint256[] calldata sigs) internal pure returns (bytes32 out) {
        uint64 a = uint64(sigs[0]);
        uint64 b = uint64(sigs[1]);
        uint64 c = uint64(sigs[2]);
        uint64 d = uint64(sigs[3]);
        out = bytes32(
            (uint256(d) << 192) |
            (uint256(c) << 128) |
            (uint256(b) << 64) |
            uint256(a)
        );
    }
}
