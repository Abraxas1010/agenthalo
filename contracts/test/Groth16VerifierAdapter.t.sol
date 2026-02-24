// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "../Groth16VerifierAdapter.sol";
import "../TrustVerifier.sol";
import "../TrustVerifierMultiChain.sol";

/// @dev Mock Groth16 verifier for adapter tests.
contract MockG16 is IGroth16Verifier {
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

/// @dev Legacy verifier ABI (3 public signals) used as mismatch fixture.
contract LegacyGroth16Verifier3Signals {
    function verifyProof(
        uint256[2] calldata,
        uint256[2][2] calldata,
        uint256[2] calldata,
        uint256[3] calldata
    ) external pure returns (bool) {
        return true;
    }
}

/// @dev Mock Groth16 verifier that validates proof values are decoded correctly.
contract AssertingG16 is IGroth16Verifier {
    uint256[2] public expectedA;
    uint256[2][2] public expectedB;
    uint256[2] public expectedC;
    uint256[6] public expectedSigs;

    function setExpected(
        uint256[2] calldata a,
        uint256[2][2] calldata b,
        uint256[2] calldata c,
        uint256[6] calldata sigs
    ) external {
        expectedA = a;
        expectedB = b;
        expectedC = c;
        expectedSigs = sigs;
    }

    function verifyProof(
        uint256[2] calldata a,
        uint256[2][2] calldata b,
        uint256[2] calldata c,
        uint256[6] calldata pubSignals
    ) external view returns (bool) {
        if (a[0] != expectedA[0] || a[1] != expectedA[1]) return false;
        if (b[0][0] != expectedB[0][0] || b[0][1] != expectedB[0][1]) return false;
        if (b[1][0] != expectedB[1][0] || b[1][1] != expectedB[1][1]) return false;
        if (c[0] != expectedC[0] || c[1] != expectedC[1]) return false;
        for (uint256 i = 0; i < 6; i++) {
            if (pubSignals[i] != expectedSigs[i]) return false;
        }
        return true;
    }
}

/// @dev Mock ERC-20 for integration tests.
contract MockTokenAdapter is IERC20 {
    mapping(address => uint256) public balance;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amt) external {
        balance[to] += amt;
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        return true;
    }

    function transferFrom(address from, address to, uint256 value) external returns (bool) {
        if (allowance[from][msg.sender] < value) return false;
        if (balance[from] < value) return false;
        allowance[from][msg.sender] -= value;
        balance[from] -= value;
        balance[to] += value;
        return true;
    }
}

contract Groth16VerifierAdapterHarness {
    MockG16 g16;
    Groth16VerifierAdapter adapter;

    MockTokenAdapter token;
    TrustVerifier tv;
    TrustVerifierMultiChain tvmc;

    constructor() {
        g16 = new MockG16();
        adapter = new Groth16VerifierAdapter(address(g16));
        token = new MockTokenAdapter();
        tv = new TrustVerifier(address(adapter), address(token), address(0xBEEF), 10);
        tvmc = new TrustVerifierMultiChain(address(adapter), address(token), address(0xBEEF), 0, 1);
    }

    function buildProof() internal pure returns (bytes memory) {
        uint256[2] memory a = [uint256(11), uint256(12)];
        uint256[2][2] memory b = [[uint256(21), uint256(22)], [uint256(23), uint256(24)]];
        uint256[2] memory c = [uint256(31), uint256(32)];
        return abi.encode(a, b, c);
    }

    function buildSignals(
        uint64 l0,
        uint64 l1,
        uint64 l2,
        uint64 l3,
        uint8 tier,
        uint64 seq
    ) internal pure returns (uint256[] memory sigs) {
        sigs = new uint256[](6);
        sigs[0] = uint256(l0);
        sigs[1] = uint256(l1);
        sigs[2] = uint256(l2);
        sigs[3] = uint256(l3);
        sigs[4] = uint256(tier);
        sigs[5] = uint256(seq);
    }

    function _selector(bytes memory reason) internal pure returns (bytes4 sel) {
        if (reason.length < 4) return bytes4(0);
        assembly {
            sel := mload(add(reason, 0x20))
        }
    }

    // 1. Happy path
    function test_adapterHappyPath() external view {
        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        require(adapter.verifyProof(proof, sigs), "adapter should accept valid mock proof");
    }

    // 2. Underlying verifier rejects
    function test_adapterRejectsWhenUnderlying() external {
        g16.setResult(false);
        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        require(!adapter.verifyProof(proof, sigs), "adapter should reflect downstream false");
        g16.setResult(true);
    }

    // 3. Proof length validation
    function test_adapterRejectsWrongProofLength() external view {
        bytes memory shortProof = new bytes(255);
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        require(!adapter.verifyProof(shortProof, sigs), "short proof should be rejected");
    }

    // 4. Too few signals
    function test_adapterRejectsFiveSignals() external view {
        bytes memory proof = buildProof();
        uint256[] memory sigs = new uint256[](5);
        sigs[0] = 1;
        sigs[1] = 2;
        sigs[2] = 3;
        sigs[3] = 4;
        sigs[4] = 1;
        require(!adapter.verifyProof(proof, sigs), "5 signals should be rejected");
    }

    // 5. Too many signals
    function test_adapterRejectsSevenSignals() external view {
        bytes memory proof = buildProof();
        uint256[] memory sigs = new uint256[](7);
        for (uint256 i = 0; i < 7; i++) sigs[i] = i + 1;
        require(!adapter.verifyProof(proof, sigs), "7 signals should be rejected");
    }

    // 6. Zero signals
    function test_adapterRejectsZeroSignals() external view {
        bytes memory proof = buildProof();
        uint256[] memory sigs = new uint256[](0);
        require(!adapter.verifyProof(proof, sigs), "0 signals should be rejected");
    }

    // 7. Proof decoding fidelity
    function test_adapterProofDecodingFidelity() external {
        AssertingG16 ag16 = new AssertingG16();
        Groth16VerifierAdapter a2 = new Groth16VerifierAdapter(address(ag16));

        uint256[2] memory a = [uint256(0xA1), uint256(0xA2)];
        uint256[2][2] memory b = [[uint256(0xB1), uint256(0xB2)], [uint256(0xB3), uint256(0xB4)]];
        uint256[2] memory c = [uint256(0xC1), uint256(0xC2)];
        uint256[6] memory expectedSigs = [uint256(10), uint256(20), uint256(30), uint256(40), uint256(2), uint256(7)];

        ag16.setExpected(a, b, c, expectedSigs);

        bytes memory proof = abi.encode(a, b, c);
        uint256[] memory sigs = new uint256[](6);
        sigs[0] = 10;
        sigs[1] = 20;
        sigs[2] = 30;
        sigs[3] = 40;
        sigs[4] = 2;
        sigs[5] = 7;

        require(a2.verifyProof(proof, sigs), "decoded proof should match expected tuple");
    }

    // 8. Constants exposed correctly
    function test_adapterConstants() external view {
        require(adapter.PROOF_LENGTH() == 256, "wrong proof length constant");
        require(adapter.SIGNAL_COUNT() == 6, "wrong signal count constant");
    }

    // 9. Integration: adapter -> TrustVerifier.attestAndPay
    function test_adapterIntegrationTrustVerifier() external {
        token.mint(address(this), 100);
        token.approve(address(tv), 100);

        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 1, 1);
        tv.attestAndPay(proof, sigs);

        (bool active, , uint8 tier, , uint64 replaySeq) = tv.agentStatus(address(this));
        require(active, "agent should be active");
        require(tier == 1, "tier mismatch");
        require(replaySeq == 1, "replay sequence mismatch");
    }

    // 10. Integration: adapter -> TrustVerifierMultiChain.submitCompositeAttestation
    function test_adapterIntegrationMultiChain() external {
        tvmc.registerChain(8453, address(0x1111), keccak256("base"));
        tvmc.setChainFee(8453, 2);
        tvmc.registerChain(1, address(0x2222), keccak256("eth"));
        tvmc.setChainFee(1, 5);

        token.mint(address(this), 100);
        token.approve(address(tvmc), 100);

        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        uint256[] memory chains = new uint256[](2);
        chains[0] = 8453;
        chains[1] = 1;

        tvmc.submitCompositeAttestation(proof, sigs, chains);

        require(tvmc.isVerifiedForChain(address(this), 8453), "base chain not verified");
        require(tvmc.isVerifiedForChain(address(this), 1), "eth chain not verified");
        require(tvmc.isVerifiedMultiChain(address(this), chains), "multichain view mismatch");
        require(token.balance(address(0xBEEF)) == 7, "fee routing mismatch");
    }

    // 11. Constructor guards: zero address and no-code address
    function test_adapterConstructorGuards() external {
        try new Groth16VerifierAdapter(address(0)) {
            revert("expected zero-address constructor revert");
        } catch (bytes memory reason0) {
            require(
                _selector(reason0) == Groth16VerifierAdapter.InvalidVerifierAddress.selector,
                "wrong revert for zero address"
            );
        }

        try new Groth16VerifierAdapter(address(0x1234)) {
            revert("expected no-code constructor revert");
        } catch (bytes memory reason1) {
            require(
                _selector(reason1) == Groth16VerifierAdapter.VerifierCodeMissing.selector,
                "wrong revert for no-code address"
            );
        }
    }

    // 12. Legacy snarkjs verifier ABI mismatch should fail closed (false, not revert)
    function test_adapterLegacyVerifierAbiMismatchFailsClosed() external {
        LegacyGroth16Verifier3Signals legacy = new LegacyGroth16Verifier3Signals();
        Groth16VerifierAdapter a3 = new Groth16VerifierAdapter(address(legacy));
        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        require(!a3.verifyProof(proof, sigs), "adapter must fail closed on ABI mismatch");
    }
}
