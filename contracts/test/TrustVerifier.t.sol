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
}
