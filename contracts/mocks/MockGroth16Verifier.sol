// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "../Groth16VerifierAdapter.sol";

/// @notice Mock implementation of IGroth16Verifier for adapter testing.
/// @dev Accepts fixed-size array parameters matching the standard snarkjs
///      Groth16 verifier output for a 6-signal circuit.
contract MockGroth16Verifier is IGroth16Verifier {
    bool public nextResult = true;

    function setResult(bool v) external {
        nextResult = v;
    }

    function verifyProof(
        uint256[2] calldata,
        uint256[2][2] calldata,
        uint256[2] calldata,
        uint256[6] calldata
    ) external view returns (bool) {
        return nextResult;
    }
}
