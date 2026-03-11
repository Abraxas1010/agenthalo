# NucleusDB Contract Scripts

This directory contains release and attestation scripts for NucleusDB trust contracts.

## AgentHALO Phase 5 Scripts

### `deploy_agenthalo_trust_base_sepolia.sh`

Deploys `TrustVerifier` for AgentHALO on Base Sepolia with:
- chain-id preflight (`84532`)
- signer selection (keystore preferred, private key fallback)
- optional Basescan verification (`BASESCAN_API_KEY`)
- JSON deployment artifact output including `script_sha256`

### `e2e_agenthalo_attestation_base_sepolia.sh`

Runs an end-to-end AgentHALO attestation flow:
- configures AgentHALO onchain settings
- creates a local test session
- submits non-anonymous and anonymous `agenthalo attest --onchain`
- checks `isVerified` and `getAttestation`
- emits JSON evidence with deployment artifact hash linkage

### `verify_agenthalo_phase5_artifacts.py`

Validates deploy + e2e artifact invariants:
- schema and chain checks
- contract/tx/digest format checks
- script hash checks
- optional strict live checks (`--require-live`)

## Multi-chain Base Sepolia Scripts

### `deploy_multichain_base_sepolia.sh`

Deploys `TrustVerifierMultiChain` to Base Sepolia, sets default fees, and registers configured chains.

### `e2e_multichain_base_sepolia.sh`

Runs live attestation flow against a deployed `TrustVerifierMultiChain` contract, validates verification views, and checks treasury fee delta.

### `deploy_mock_e2e_base_sepolia.sh`

Runs a full one-command mock-backed E2E validation on Base Sepolia:
- deploy `MockTrustProofVerifier`
- deploy `MockUSDC` and mint test funds
- deploy `TrustVerifierMultiChain` wired to mocks
- register chains + tiered fees
- run single and multichain attestations
- validate verification views and treasury economics
- write JSON evidence

## Environment Variables

| Variable | Deploy | E2E | Required | Notes |
|---|---|---|---|---|
| `RPC_URL_BASE_SEPOLIA` | yes | yes | yes | Must resolve to chain id `84532`. |
| `TRUST_VERIFIER_GROTH16_VERIFIER` | yes | no | deploy only | Groth16 verifier address for constructor. |
| `TRUST_TREASURY` | yes | no | deploy only | Treasury address for constructor. E2E reads from contract if unset. |
| `TRUST_VERIFIER_MULTI_CHAIN_ADDRESS` | no | yes | e2e only | Address emitted by deploy script. |
| `AGENT_ADDRESS` | no | yes | e2e only | Agent whose chain verification is checked. |
| `ETH_KEYSTORE` | yes | yes | signer | Preferred signer mode. |
| `ETH_PASSWORD_FILE` | yes | yes | signer | Password file for keystore mode. |
| `PRIVATE_KEY` | yes | yes | signer fallback | Used only when keystore mode is not configured. |
| `BASESCAN_API_KEY` | yes | no | no | If set, deploy uses `forge create --verify`. |
| `DEPLOYMENT_OUT` | yes | yes | no | Deploy writes JSON evidence; E2E uses file hash if path exists. |
| `EVIDENCE_OUT` | no | yes | no | E2E JSON evidence output path. |
| `USDC_BASE_SEPOLIA` | yes | yes | no | Deploy default is Base Sepolia USDC; E2E reads from contract if unset. |

## AgentHALO Phase 5 Environment Variables

| Variable | Deploy | E2E | Required | Notes |
|---|---|---|---|---|
| `RPC_URL_BASE_SEPOLIA` | yes | yes | yes | Must resolve to chain id `84532`. |
| `TRUST_VERIFIER_GROTH16_VERIFIER` | yes | no | deploy only | Constructor verifier address for `TrustVerifier`. |
| `TRUST_TREASURY` | yes | no | deploy only | Constructor treasury address. |
| `TRUST_VERIFIER_ADDRESS` | no | yes | e2e only | Deployed `TrustVerifier` address under test. |
| `AGENTHALO_BIN` | no | yes | no | Defaults to `target/debug/agenthalo` then `target/release/agenthalo`. |
| `AGENTHALO_ONCHAIN_SIMULATION` | no | yes | no | Defaults to `1` for local/no-key smoke runs. |
| `ETH_KEYSTORE` + `ETH_PASSWORD_FILE` | yes | yes | signer | Preferred signer mode. |
| `PRIVATE_KEY` / `AGENTHALO_ONCHAIN_PRIVATE_KEY` | yes | yes | signer fallback | Used only when keystore is not configured. |
| `BASESCAN_API_KEY` | yes | no | no | Enables `forge create --verify`. |
| `DEPLOYMENT_OUT` | yes | yes | no | Deployment artifact path and hash-link source. |
| `EVIDENCE_OUT` | no | yes | no | E2E evidence output path. |

## Mock E2E Environment Variables (`deploy_mock_e2e_base_sepolia.sh`)

| Variable | Required | Default | Notes |
|---|---|---|---|
| `RPC_URL_BASE_SEPOLIA` | no | `https://sepolia.base.org` | Must resolve to chain id `84532`. |
| `PRIVATE_KEY` | conditional | from `.env.testnet` at repo root | Required if not present in default env file. |
| `AGENT_PRIVATE_KEY` | no | `PRIVATE_KEY` | Agent signer key for attestation submission. |
| `AGENT_ADDRESS` | no | derived from `AGENT_PRIVATE_KEY` | Must match `AGENT_PRIVATE_KEY` when explicitly set. |
| `TREASURY_ADDRESS` | no | `0x1111111111111111111111111111111111111111` | Treasury receives attestation fees. |
| `TX_GAS_PRICE_WEI` | no | `6000000` | Base gas-price floor for retry ladder. |
| `MOCK_MINT_AMOUNT` | no | `100000000` | Mock USDC mint amount for deployer. |
| `TRUST_LEGACY_FEE_WEI` | no | `0` | Constructor legacy fee arg (unused by multichain economics). |
| `TRUST_DEFAULT_FEE_WEI` | no | `1000000` | Default fallback fee when chain-specific fee is not set. |
| `TRUST_BASE_CHAIN_FEE_WEI` | no | `1000000` | Fee configured for chain `8453`. |
| `TRUST_ETH_CHAIN_FEE_WEI` | no | `5000000` | Fee configured for chain `1`. |
| `EXPECT_TREASURY_ABS_BALANCE` | no | `7000000` | Expected final treasury balance after two attestations. |
| `PROOF_HEX_SINGLE` | no | `0x01` | Single-chain proof payload for mock verifier. |
| `PROOF_HEX_MULTI` | no | `0x01` | Multi-chain proof payload for mock verifier. |
| `PUBLIC_SIGNALS_SINGLE` | no | `[1,2,3,4,2,1]` | Single-chain public signal vector. |
| `PUBLIC_SIGNALS_MULTI` | no | `[1,2,3,4,2,2]` | Multi-chain public signal vector (replay seq increases). |
| `CHAINS_SINGLE` | no | `[8453]` | Single-chain attestation path. |
| `CHAINS_MULTI` | no | `[8453,1]` | Multi-chain attestation path. |
| `REQUIRED_MULTI_PASS` | no | `[8453,1]` | Positive multichain verification check set. |
| `REQUIRED_MULTI_FAIL` | no | `[8453,42161]` | Negative multichain verification check set. |
| `EVIDENCE_OUT` | no | unset | If set, writes JSON evidence with `script_sha256`. |

## Production Deployment with Groth16VerifierAdapter

For production use, deploy the `Groth16VerifierAdapter` to bridge a snarkjs-generated Groth16 verifier to the `ITrustProofVerifier` interface:

```bash
cd contracts

# 1. Generate verifier from circuit (see circuits/README.md)
#    This produces Groth16TrustVerifier.sol with verifyProof(uint256[2], uint256[2][2], uint256[2], uint256[6])

# 2. Deploy the snarkjs-generated verifier
forge create Groth16TrustVerifier --rpc-url $RPC_URL --private-key $PK
# → GROTH16_ADDRESS=0x...

# 3. Deploy the adapter (wraps Groth16 verifier into ITrustProofVerifier)
forge create Groth16VerifierAdapter --constructor-args $GROTH16_ADDRESS \
  --rpc-url $RPC_URL --private-key $PK
# → ADAPTER_ADDRESS=0x...

# 4. Deploy TrustVerifierMultiChain with the adapter as verifier
forge create TrustVerifierMultiChain \
  --constructor-args $ADAPTER_ADDRESS $USDC_ADDRESS $TREASURY $FEE $DEFAULT_FEE \
  --rpc-url $RPC_URL --private-key $PK
```

The adapter encodes proofs as `abi.encode(uint256[2] a, uint256[2][2] b, uint256[2] c)` (256 bytes) and expects exactly 6 public signals matching the `[pufDigest_limb0..3, tier, replaySeq]` convention.

## Example Invocation Sequence

```bash
cd contracts

export RPC_URL_BASE_SEPOLIA="..."
export TRUST_VERIFIER_GROTH16_VERIFIER="0x..."
export TRUST_TREASURY="0x..."
export ETH_KEYSTORE="$HOME/.foundry/keystores/deployer.json"
export ETH_PASSWORD_FILE="$HOME/.foundry/keystores/deployer.password"
export DEPLOYMENT_OUT="artifacts/ops/multichain_sepolia/deploy_report.json"
# Optional:
# export BASESCAN_API_KEY="..."

./scripts/deploy_multichain_base_sepolia.sh

export TRUST_VERIFIER_MULTI_CHAIN_ADDRESS="0x..."
export AGENT_ADDRESS="0x..."
export EVIDENCE_OUT="artifacts/ops/multichain_sepolia/e2e_report.json"

./scripts/e2e_multichain_base_sepolia.sh
```

Mock one-command E2E:

```bash
cd contracts
export EVIDENCE_OUT="artifacts/ops/multichain_sepolia/mock_e2e_$(date -u +%Y%m%dT%H%M%SZ).json"
./scripts/deploy_mock_e2e_base_sepolia.sh
```

## Expected Outputs

- Deploy script:
  - stdout summary with deployed contract and tx hashes
  - optional JSON file at `DEPLOYMENT_OUT` including `script_sha256`
- E2E script:
  - stdout summary with attestation tx hashes and verification checks
  - optional JSON file at `EVIDENCE_OUT` including:
    - `script_sha256`
    - `deployment_evidence_sha256` (when `DEPLOYMENT_OUT` is provided and exists)
- Mock E2E script:
  - stdout summary with deployed mock addresses, attestation tx hashes, verification checks, and treasury delta
  - optional JSON file at `EVIDENCE_OUT` including:
    - all deployed addresses
    - all tx hashes
    - positive/negative verification outcomes
    - treasury economics check
    - `script_sha256`
