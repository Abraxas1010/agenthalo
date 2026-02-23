# NucleusDB Contract Scripts

This directory contains release and attestation scripts for NucleusDB trust contracts.

## Multi-chain Base Sepolia Scripts

### `deploy_multichain_base_sepolia.sh`

Deploys `TrustVerifierMultiChain` to Base Sepolia, sets default fees, and registers configured chains.

### `e2e_multichain_base_sepolia.sh`

Runs live attestation flow against a deployed `TrustVerifierMultiChain` contract, validates verification views, and checks treasury fee delta.

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

## Example Invocation Sequence

```bash
cd projects/nucleusdb/contracts

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

## Expected Outputs

- Deploy script:
  - stdout summary with deployed contract and tx hashes
  - optional JSON file at `DEPLOYMENT_OUT` including `script_sha256`
- E2E script:
  - stdout summary with attestation tx hashes and verification checks
  - optional JSON file at `EVIDENCE_OUT` including:
    - `script_sha256`
    - `deployment_evidence_sha256` (when `DEPLOYMENT_OUT` is provided and exists)
