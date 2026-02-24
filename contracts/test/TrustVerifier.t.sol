// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "../TrustVerifier.sol";

contract MockToken is IERC20 {
    mapping(address => uint256) public balance;
    mapping(address => mapping(address => uint256)) public allowance;
    bool public forceFailTransferFrom;

    function mint(address to, uint256 amt) external {
        balance[to] += amt;
    }

    function setForceFailTransferFrom(bool v) external {
        forceFailTransferFrom = v;
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        return true;
    }

    function transferFrom(address from, address to, uint256 value) external returns (bool) {
        if (forceFailTransferFrom) return false;
        if (allowance[from][msg.sender] < value) return false;
        if (balance[from] < value) return false;
        allowance[from][msg.sender] -= value;
        balance[from] -= value;
        balance[to] += value;
        return true;
    }
}

contract MockVerifier is ITrustProofVerifier {
    bool public nextResult = true;

    function setResult(bool v) external {
        nextResult = v;
    }

    function verifyProof(bytes calldata, uint256[] calldata) external view returns (bool) {
        return nextResult;
    }
}

contract TrustVerifierHarness {
    MockToken token;
    MockVerifier verifier;
    TrustVerifier tv;

    constructor() {
        token = new MockToken();
        verifier = new MockVerifier();
        tv = new TrustVerifier(address(verifier), address(token), address(this), 10);
    }

    function buildSignals(
        uint64 a,
        uint64 b,
        uint64 c,
        uint64 d,
        uint8 tier,
        uint64 seq
    ) internal pure returns (uint256[] memory sigs) {
        sigs = new uint256[](6);
        sigs[0] = uint256(a);
        sigs[1] = uint256(b);
        sigs[2] = uint256(c);
        sigs[3] = uint256(d);
        sigs[4] = uint256(tier);
        sigs[5] = uint256(seq);
    }

    function buildAttestationInputs(
        uint128 merkleLo,
        uint128 merkleHi,
        uint128 digestLo,
        uint128 digestHi,
        uint64 eventCount
    ) internal pure returns (uint256[] memory inputs) {
        inputs = new uint256[](5);
        inputs[0] = uint256(merkleLo);
        inputs[1] = uint256(merkleHi);
        inputs[2] = uint256(digestLo);
        inputs[3] = uint256(digestHi);
        inputs[4] = uint256(eventCount);
    }

    function testAttestAndPayHappyPath() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 1, 1);
        tv.attestAndPay(proof, sigs);
        (
            bool active,
            bytes32 digest,
            uint8 tier,
            uint64 lastAttestation,
            uint64 replaySeq
        ) = tv.agentStatus(address(this));
        return active && digest != bytes32(0) && tier == 1 && lastAttestation > 0 && replaySeq == 1;
    }

    function testVerifyAgentFalseBeforeAttest() external view returns (bool) {
        return !tv.verifyAgent(address(this));
    }

    function testRoutesFeeToTreasury() external returns (bool) {
        address treasury = address(0xBEEF);
        TrustVerifier tv2 = new TrustVerifier(address(verifier), address(token), treasury, 10);
        token.mint(address(this), 100);
        token.approve(address(tv2), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 1, 1);
        tv2.attestAndPay(proof, sigs);
        return token.balance(treasury) == 10;
    }

    function testRejectsInvalidProof() external returns (bool) {
        verifier.setResult(false);
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 1, 1);
        try tv.attestAndPay(proof, sigs) {
            return false;
        } catch {
            return true;
        }
    }

    function testRejectsShortSignals() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = new uint256[](5);
        sigs[0] = 1;
        sigs[1] = 2;
        sigs[2] = 3;
        sigs[3] = 4;
        sigs[4] = 1;
        try tv.attestAndPay(proof, sigs) {
            return false;
        } catch {
            return true;
        }
    }

    function testRejectsZeroTier() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 0, 1);
        try tv.attestAndPay(proof, sigs) {
            return false;
        } catch {
            return true;
        }
    }

    function testRejectsTierOutOfRange() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 5, 1);
        try tv.attestAndPay(proof, sigs) {
            return false;
        } catch {
            return true;
        }
    }

    function testRejectsZeroDigest() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(0, 0, 0, 0, 1, 1);
        try tv.attestAndPay(proof, sigs) {
            return false;
        } catch {
            return true;
        }
    }

    function testRejectsTransferFailure() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 10);
        token.setForceFailTransferFrom(true);
        bytes memory proof = hex"01";
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 1, 1);
        try tv.attestAndPay(proof, sigs) {
            return false;
        } catch {
            return true;
        }
    }

    function testRejectsSequenceRegression() external returns (bool) {
        token.mint(address(this), 200);
        token.approve(address(tv), 200);
        bytes memory proof = hex"01";
        uint256[] memory sigs1 = buildSignals(1, 2, 3, 4, 1, 10);
        tv.attestAndPay(proof, sigs1);

        uint256[] memory sigs2 = buildSignals(1, 2, 3, 4, 1, 9);
        try tv.attestAndPay(proof, sigs2) {
            return false;
        } catch {
            return true;
        }
    }

    function testAllowsSequenceAdvance() external returns (bool) {
        token.mint(address(this), 200);
        token.approve(address(tv), 200);
        bytes memory proof = hex"01";
        uint256[] memory sigs1 = buildSignals(1, 2, 3, 4, 2, 11);
        tv.attestAndPay(proof, sigs1);

        uint256[] memory sigs2 = buildSignals(1, 2, 3, 4, 2, 12);
        tv.attestAndPay(proof, sigs2);
        (, , , , uint64 replaySeq) = tv.agentStatus(address(this));
        return replaySeq == 12;
    }

    function testVerifyAndRecordHappyPath() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 100);
        uint256[8] memory proofWords = [uint256(1), 2, 3, 4, 5, 6, 7, 8];
        uint256[] memory inputs = buildAttestationInputs(11, 12, 21, 22, 2);
        tv.verifyAndRecord(proofWords, inputs);
        bytes32 digest = bytes32((uint256(22) << 128) | uint256(21));
        return tv.isVerified(digest);
    }

    function testVerifyAndRecordRejectsMalformedInputLength() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 100);
        uint256[8] memory proofWords = [uint256(1), 2, 3, 4, 5, 6, 7, 8];
        uint256[] memory malformed = new uint256[](4);
        malformed[0] = 11;
        malformed[1] = 12;
        malformed[2] = 21;
        malformed[3] = 22;
        try tv.verifyAndRecord(proofWords, malformed) {
            return false;
        } catch {
            return true;
        }
    }

    function testVerifyAndRecordRejectsDuplicate() external returns (bool) {
        token.mint(address(this), 200);
        token.approve(address(tv), 200);
        uint256[8] memory proofWords = [uint256(1), 2, 3, 4, 5, 6, 7, 8];
        uint256[] memory inputs = buildAttestationInputs(11, 12, 31, 32, 1);
        tv.verifyAndRecord(proofWords, inputs);
        try tv.verifyAndRecord(proofWords, inputs) {
            return false;
        } catch {
            return true;
        }
    }

    function testVerifyAndRecordOrderingMismatchNegative() external returns (bool) {
        token.mint(address(this), 200);
        token.approve(address(tv), 200);
        uint256[8] memory proofWords = [uint256(1), 2, 3, 4, 5, 6, 7, 8];
        uint256[] memory canonical = buildAttestationInputs(101, 102, 201, 202, 3);
        uint256[] memory swapped = new uint256[](5);
        swapped[0] = canonical[2];
        swapped[1] = canonical[3];
        swapped[2] = canonical[0];
        swapped[3] = canonical[1];
        swapped[4] = canonical[4];

        tv.verifyAndRecord(proofWords, swapped);

        bytes32 canonicalDigest = bytes32((uint256(canonical[3]) << 128) | uint256(canonical[2]));
        bytes32 swappedDigest = bytes32((uint256(swapped[3]) << 128) | uint256(swapped[2]));
        bool canonicalVerified = tv.isVerified(canonicalDigest);
        bool swappedVerified = tv.isVerified(swappedDigest);
        return !canonicalVerified && swappedVerified;
    }

    function testVerifyAndRecordAnonymousMasksAttester() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 100);
        uint256[8] memory proofWords = [uint256(1), 2, 3, 4, 5, 6, 7, 8];
        uint256[] memory inputs = buildAttestationInputs(41, 42, 51, 52, 4);
        tv.verifyAndRecordAnonymous(proofWords, inputs);
        bytes32 digest = bytes32((uint256(52) << 128) | uint256(51));
        TrustVerifier.AttestationRecord memory rec = tv.getAttestation(digest);
        return rec.verified && rec.eventCount == 4 && rec.attester == address(0);
    }
}
