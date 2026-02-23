// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

import "../TrustVerifierMultiChain.sol";

contract MockTokenMC is IERC20 {
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

contract MockVerifierMC is ITrustProofVerifier {
    bool public nextResult = true;

    function setResult(bool v) external {
        nextResult = v;
    }

    function verifyProof(bytes calldata, uint256[] calldata) external view returns (bool) {
        return nextResult;
    }
}

contract TrustVerifierMultiChainHarness {
    MockTokenMC token;
    MockVerifierMC verifier;
    TrustVerifierMultiChain tv;
    address treasury = address(0xBEEF);

    constructor() {
        token = new MockTokenMC();
        verifier = new MockVerifierMC();
        tv = new TrustVerifierMultiChain(address(verifier), address(token), treasury, 0, 1);
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

    function makeRegistered(uint256 chainId, uint256 fee) internal {
        tv.registerChain(chainId, address(0x1111), keccak256(abi.encode(chainId)));
        tv.setChainFee(chainId, fee);
    }

    function prepFee(uint256 amount) internal {
        token.mint(address(this), amount);
        token.approve(address(tv), amount);
    }

    function test_registerChain() external returns (bool) {
        tv.registerChain(8453, address(0x1111), keccak256("base"));
        (bool registered, address verifierAddress, bytes32 metadataHash, uint64 registeredAt, ) = tv
            .chainInfo(8453);
        return registered && verifierAddress == address(0x1111) && metadataHash != bytes32(0) && registeredAt > 0;
    }

    function test_setChainFee() external returns (bool) {
        tv.registerChain(8453, address(0x1111), keccak256("base"));
        tv.setChainFee(8453, 7);
        return tv.chainFees(8453) == 7;
    }

    function test_submitCompositeAttestation_singleChain() external returns (bool) {
        makeRegistered(8453, 2);
        prepFee(10);
        uint256[] memory chains = new uint256[](1);
        chains[0] = 8453;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), chains);
        return tv.verifyAgent(address(this)) && tv.isVerifiedForChain(address(this), 8453);
    }

    function test_submitCompositeAttestation_multiChain() external returns (bool) {
        makeRegistered(8453, 1);
        makeRegistered(1, 5);
        prepFee(20);
        uint256[] memory chains = new uint256[](2);
        chains[0] = 8453;
        chains[1] = 1;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 2, 1), chains);
        return tv.isVerifiedMultiChain(address(this), chains);
    }

    function test_tieredFeeCalculation() external returns (bool) {
        makeRegistered(8453, 1);
        makeRegistered(1, 5);
        makeRegistered(42161, 2);
        prepFee(100);
        uint256[] memory chains = new uint256[](3);
        chains[0] = 8453;
        chains[1] = 1;
        chains[2] = 42161;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 2, 1), chains);
        return token.balance(treasury) == 8;
    }

    function test_defaultFeeFallback() external returns (bool) {
        tv.setDefaultFee(4);
        tv.registerChain(10, address(0x2222), keccak256("op"));
        prepFee(10);
        uint256[] memory chains = new uint256[](1);
        chains[0] = 10;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), chains);
        return token.balance(treasury) == 4;
    }

    function test_rejectUnregisteredChain() external returns (bool) {
        makeRegistered(8453, 1);
        prepFee(10);
        uint256[] memory chains = new uint256[](2);
        chains[0] = 8453;
        chains[1] = 777;
        try tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), chains) {
            return false;
        } catch {
            return true;
        }
    }

    function test_rejectTooManyChains() external returns (bool) {
        prepFee(1000);
        uint256[] memory chains = new uint256[](9);
        for (uint256 i = 0; i < chains.length; i++) {
            uint256 chainId = 100 + i;
            tv.registerChain(chainId, address(0x1111), keccak256(abi.encode(chainId)));
            tv.setChainFee(chainId, 1);
            chains[i] = chainId;
        }
        try tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), chains) {
            return false;
        } catch {
            return true;
        }
    }

    function test_isVerifiedForChain() external returns (bool) {
        makeRegistered(8453, 1);
        prepFee(10);
        uint256[] memory chains = new uint256[](1);
        chains[0] = 8453;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), chains);
        return tv.isVerifiedForChain(address(this), 8453) && !tv.isVerifiedForChain(address(this), 9999);
    }

    function test_isVerifiedMultiChain() external returns (bool) {
        makeRegistered(8453, 1);
        makeRegistered(42161, 1);
        prepFee(10);
        uint256[] memory chains = new uint256[](2);
        chains[0] = 8453;
        chains[1] = 42161;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), chains);
        uint256[] memory required = new uint256[](2);
        required[0] = 8453;
        required[1] = 42161;
        uint256[] memory mixed = new uint256[](2);
        mixed[0] = 8453;
        mixed[1] = 10;
        return tv.isVerifiedMultiChain(address(this), required) && !tv.isVerifiedMultiChain(address(this), mixed);
    }

    function test_compositeOverwritesPrevious() external returns (bool) {
        makeRegistered(8453, 1);
        makeRegistered(1, 1);
        prepFee(20);
        uint256[] memory first = new uint256[](1);
        first[0] = 8453;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 1), first);

        uint256[] memory second = new uint256[](2);
        second[0] = 8453;
        second[1] = 1;
        tv.submitCompositeAttestation(hex"01", buildSignals(1, 2, 3, 4, 1, 2), second);
        (, uint64 timestamp, bool valid, uint256 chainCount) = tv.getCompositeAttestation(address(this));
        uint256[] memory chains = tv.getCompositeChains(address(this));
        return valid && timestamp > 0 && chainCount == 2 && chains.length == 2;
    }
}
