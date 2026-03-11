// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

/// @title Cross-chain attestation query interface
/// @notice Bridge-agnostic interface for requesting and receiving remote-chain verification.
interface ICrossChainAttestationQuery {
    /// @notice Request verification status of an agent from another chain.
    function requestCrossChainVerification(
        uint256 sourceChainId,
        uint256 targetChainId,
        address agent,
        bytes32 requestId
    ) external;

    /// @notice Receive verification result from a remote chain.
    function receiveCrossChainVerification(
        uint256 sourceChainId,
        uint256 targetChainId,
        address agent,
        bool verified,
        bytes32 requestId
    ) external;
}
