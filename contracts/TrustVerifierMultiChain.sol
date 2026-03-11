// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "./TrustVerifier.sol";

/// @title NucleusDB TrustVerifier (Multi-Chain)
/// @notice Extends trust attestation with chain registry and tiered per-chain fees.
contract TrustVerifierMultiChain is TrustVerifier {
    error Unauthorized();
    error EmptyChainSet();
    error TooManyChains(uint256 provided, uint256 maxAllowed);
    error ChainNotRegistered(uint256 chainId);
    error InvalidMetadataHash();

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event ChainRegistered(
        uint256 indexed chainId,
        address indexed verifierAddress,
        bytes32 indexed metadataHash,
        uint64 registeredAt
    );
    event ChainFeeUpdated(uint256 indexed chainId, uint256 fee);
    event DefaultFeeUpdated(uint256 fee);
    event ChainVerificationUpdated(address indexed agent, uint256 indexed chainId, bool verified);
    event CompositeAttestationSubmitted(
        address indexed agent,
        bytes32 indexed compositeCabHash,
        uint64 indexed timestamp,
        uint256 totalFee,
        uint256 chainCount
    );

    struct ChainRegistration {
        bool registered;
        address verifierAddress;
        bytes32 metadataHash;
        uint64 registeredAt;
    }

    struct CompositeAttestation {
        address agent;
        uint256[] chains;
        bytes32 compositeCabHash;
        uint64 timestamp;
        bool valid;
    }

    address public owner;
    mapping(uint256 => ChainRegistration) public chainRegistry;
    mapping(uint256 => uint256) public chainFees;
    uint256 public defaultFee;

    mapping(address => mapping(uint256 => bool)) private chainVerification;
    mapping(address => CompositeAttestation) private compositeAttestations;
    mapping(uint256 => bool) private chainSeen;
    uint256[] public registeredChains;
    uint256 public constant MAX_CHAINS_PER_ATTESTATION = 8;

    modifier onlyOwner() {
        if (msg.sender != owner) revert Unauthorized();
        _;
    }

    constructor(
        address verifier_,
        address usdc_,
        address treasury_,
        uint256 feeWei_,
        uint256 defaultFee_
    ) TrustVerifier(verifier_, usdc_, treasury_, feeWei_) {
        owner = msg.sender;
        defaultFee = defaultFee_;
        emit OwnershipTransferred(address(0), msg.sender);
    }

    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert Unauthorized();
        emit OwnershipTransferred(owner, newOwner);
        owner = newOwner;
    }

    /// @notice Register or update a supported chain for composite attestations.
    function registerChain(
        uint256 chainId,
        address verifierAddress,
        bytes32 metadataHash
    ) external onlyOwner {
        if (verifierAddress == address(0)) revert Unauthorized();
        if (metadataHash == bytes32(0)) revert InvalidMetadataHash();
        if (!chainSeen[chainId]) {
            chainSeen[chainId] = true;
            registeredChains.push(chainId);
        }
        chainRegistry[chainId] = ChainRegistration({
            registered: true,
            verifierAddress: verifierAddress,
            metadataHash: metadataHash,
            registeredAt: uint64(block.timestamp)
        });
        emit ChainRegistered(chainId, verifierAddress, metadataHash, uint64(block.timestamp));
    }

    function setChainFee(uint256 chainId, uint256 fee) external onlyOwner {
        chainFees[chainId] = fee;
        emit ChainFeeUpdated(chainId, fee);
    }

    function setDefaultFee(uint256 fee) external onlyOwner {
        defaultFee = fee;
        emit DefaultFeeUpdated(fee);
    }

    /// @notice Submit a composite multi-chain attestation with tiered fee settlement.
    /// @dev Chain verification is additive by design: submitting a new subset of chains
    /// does not revoke chains previously verified for the same agent.
    /// @dev Uses the same public signal convention as `attestAndPay`.
    function submitCompositeAttestation(
        bytes calldata proof,
        uint256[] calldata publicSignals,
        uint256[] calldata chains
    ) external {
        if (chains.length == 0) revert EmptyChainSet();
        if (chains.length > MAX_CHAINS_PER_ATTESTATION) {
            revert TooManyChains(chains.length, MAX_CHAINS_PER_ATTESTATION);
        }
        if (publicSignals.length < 6) revert InvalidDigest();
        if (!verifier.verifyProof(proof, publicSignals)) revert InvalidProof();

        bytes32 pufDigest = _decodeDigest(publicSignals);
        if (pufDigest == bytes32(0)) revert InvalidDigest();
        uint8 tier = uint8(publicSignals[4]);
        if (tier == 0 || tier > 4) revert InvalidTier();
        uint64 replaySeq = uint64(publicSignals[5]);

        AgentRecord memory prev = registry[msg.sender];
        if (prev.active && replaySeq <= prev.lastReplaySeq) revert SequenceRegression();

        uint256 totalFee = 0;
        for (uint256 i = 0; i < chains.length; i++) {
            uint256 chainId = chains[i];
            if (!chainRegistry[chainId].registered) revert ChainNotRegistered(chainId);
            chainVerification[msg.sender][chainId] = true;
            uint256 fee = chainFees[chainId];
            if (fee == 0) {
                fee = defaultFee;
            }
            totalFee += fee;
            emit ChainVerificationUpdated(msg.sender, chainId, true);
        }

        registry[msg.sender] = AgentRecord({
            pufDigest: pufDigest,
            lastAttestation: uint64(block.timestamp),
            lastReplaySeq: replaySeq,
            tier: tier,
            active: true
        });

        if (totalFee > 0) {
            bool ok = usdc.transferFrom(msg.sender, treasury, totalFee);
            if (!ok) revert TransferFailed();
        }

        bytes32 compositeCabHash = keccak256(
            abi.encode(msg.sender, pufDigest, tier, replaySeq, chains, publicSignals)
        );
        CompositeAttestation storage att = compositeAttestations[msg.sender];
        delete att.chains;
        att.agent = msg.sender;
        att.compositeCabHash = compositeCabHash;
        att.timestamp = uint64(block.timestamp);
        att.valid = true;
        for (uint256 i = 0; i < chains.length; i++) {
            att.chains.push(chains[i]);
        }

        emit CompositeAttestationSubmitted(
            msg.sender,
            compositeCabHash,
            uint64(block.timestamp),
            totalFee,
            chains.length
        );
        emit AgentCertified(
            msg.sender,
            pufDigest,
            tier,
            uint64(block.timestamp),
            replaySeq,
            totalFee
        );
    }

    /// @notice Returns whether agent has an active attestation for a specific chain.
    function isVerifiedForChain(address agent, uint256 chainId) external view returns (bool) {
        return registry[agent].active && chainVerification[agent][chainId];
    }

    /// @notice Returns whether agent is active and verified for all required chains.
    function isVerifiedMultiChain(
        address agent,
        uint256[] calldata requiredChains
    ) external view returns (bool) {
        if (!registry[agent].active) return false;
        for (uint256 i = 0; i < requiredChains.length; i++) {
            if (!chainVerification[agent][requiredChains[i]]) {
                return false;
            }
        }
        return true;
    }

    function registeredChainsLength() external view returns (uint256) {
        return registeredChains.length;
    }

    function getRegisteredChains() external view returns (uint256[] memory) {
        return registeredChains;
    }

    function getCompositeAttestation(
        address agent
    ) external view returns (bytes32 compositeCabHash, uint64 timestamp, bool valid, uint256 chainCount) {
        CompositeAttestation storage att = compositeAttestations[agent];
        return (att.compositeCabHash, att.timestamp, att.valid, att.chains.length);
    }

    function getCompositeChains(address agent) external view returns (uint256[] memory) {
        return compositeAttestations[agent].chains;
    }

    function chainInfo(
        uint256 chainId
    ) external view returns (bool registered, address verifierAddress, bytes32 metadataHash, uint64 registeredAt, uint256 fee) {
        ChainRegistration memory info = chainRegistry[chainId];
        return (info.registered, info.verifierAddress, info.metadataHash, info.registeredAt, chainFees[chainId]);
    }
}
