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

/// @dev Mock Groth16 verifier that validates proof values are decoded correctly.
contract AssertingG16 is IGroth16Verifier {
    uint256[2] public expectedA;
    uint256[2][2] public expectedB;
    uint256[2] public expectedC;
    uint256[6] public expectedSigs;
    bool public wasCalled;

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
        // Verify a
        if (a[0] != expectedA[0] || a[1] != expectedA[1]) return false;
        // Verify b
        if (b[0][0] != expectedB[0][0] || b[0][1] != expectedB[0][1]) return false;
        if (b[1][0] != expectedB[1][0] || b[1][1] != expectedB[1][1]) return false;
        // Verify c
        if (c[0] != expectedC[0] || c[1] != expectedC[1]) return false;
        // Verify signals
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

    // ── 1. Happy path ──

    function test_adapterHappyPath() external view returns (bool) {
        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        return adapter.verifyProof(proof, sigs);
    }

    // ── 2. Underlying verifier rejects ──

    function test_adapterRejectsWhenUnderlying() external returns (bool) {
        g16.setResult(false);
        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        bool result = adapter.verifyProof(proof, sigs);
        g16.setResult(true); // reset
        return !result;
    }

    // ── 3. Short proof ──

    function test_adapterRejectsShortProof() external view returns (bool) {
        bytes memory shortProof = new bytes(255);
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        return !adapter.verifyProof(shortProof, sigs);
    }

    // ── 4. Long proof ──

    function test_adapterRejectsLongProof() external view returns (bool) {
        bytes memory longProof = new bytes(257);
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        return !adapter.verifyProof(longProof, sigs);
    }

    // ── 5. Empty proof ──

    function test_adapterRejectsEmptyProof() external view returns (bool) {
        bytes memory emptyProof = new bytes(0);
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 2, 1);
        return !adapter.verifyProof(emptyProof, sigs);
    }

    // ── 6. Too few signals ──

    function test_adapterRejectsFiveSignals() external view returns (bool) {
        bytes memory proof = buildProof();
        uint256[] memory sigs = new uint256[](5);
        sigs[0] = 1; sigs[1] = 2; sigs[2] = 3; sigs[3] = 4; sigs[4] = 1;
        return !adapter.verifyProof(proof, sigs);
    }

    // ── 7. Too many signals ──

    function test_adapterRejectsSevenSignals() external view returns (bool) {
        bytes memory proof = buildProof();
        uint256[] memory sigs = new uint256[](7);
        for (uint256 i = 0; i < 7; i++) sigs[i] = i + 1;
        return !adapter.verifyProof(proof, sigs);
    }

    // ── 8. Zero signals ──

    function test_adapterRejectsZeroSignals() external view returns (bool) {
        bytes memory proof = buildProof();
        uint256[] memory sigs = new uint256[](0);
        return !adapter.verifyProof(proof, sigs);
    }

    // ── 9. Proof encoding fidelity ──

    function test_adapterProofDecodingFidelity() external returns (bool) {
        AssertingG16 ag16 = new AssertingG16();
        Groth16VerifierAdapter a2 = new Groth16VerifierAdapter(address(ag16));

        uint256[2] memory a = [uint256(0xA1), uint256(0xA2)];
        uint256[2][2] memory b = [[uint256(0xB1), uint256(0xB2)], [uint256(0xB3), uint256(0xB4)]];
        uint256[2] memory c = [uint256(0xC1), uint256(0xC2)];
        uint256[6] memory expectedSigs = [uint256(10), uint256(20), uint256(30), uint256(40), uint256(2), uint256(7)];

        ag16.setExpected(a, b, c, expectedSigs);

        bytes memory proof = abi.encode(a, b, c);
        uint256[] memory sigs = new uint256[](6);
        sigs[0] = 10; sigs[1] = 20; sigs[2] = 30; sigs[3] = 40; sigs[4] = 2; sigs[5] = 7;

        return a2.verifyProof(proof, sigs);
    }

    // ── 10. Constants exposed correctly ──

    function test_adapterConstants() external view returns (bool) {
        return adapter.PROOF_LENGTH() == 256 && adapter.SIGNAL_COUNT() == 6;
    }

    // ── 11. Integration: adapter → TrustVerifier.attestAndPay ──

    function test_adapterIntegrationTrustVerifier() external returns (bool) {
        token.mint(address(this), 100);
        token.approve(address(tv), 100);

        bytes memory proof = buildProof();
        uint256[] memory sigs = buildSignals(1, 2, 3, 4, 1, 1);
        tv.attestAndPay(proof, sigs);

        (bool active, , uint8 tier, , uint64 replaySeq) = tv.agentStatus(address(this));
        return active && tier == 1 && replaySeq == 1;
    }

    // ── 12. Integration: adapter → TrustVerifierMultiChain.submitCompositeAttestation ──

    function test_adapterIntegrationMultiChain() external returns (bool) {
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

        return tvmc.isVerifiedForChain(address(this), 8453) &&
               tvmc.isVerifiedForChain(address(this), 1) &&
               tvmc.isVerifiedMultiChain(address(this), chains) &&
               token.balance(address(0xBEEF)) == 7;
    }
}
