// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "../TrustVerifier.sol";

contract MockTrustProofVerifier is ITrustProofVerifier {
    bool public nextResult = true;

    function setResult(bool result) external {
        nextResult = result;
    }

    function verifyProof(bytes calldata, uint256[] calldata) external view returns (bool) {
        return nextResult;
    }
}
