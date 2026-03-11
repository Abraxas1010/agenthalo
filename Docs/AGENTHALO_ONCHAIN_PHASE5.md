# AgentHALO On-Chain Phase 5 Runbook

## Scope

Phase 5 hardens AgentHALO Groth16 on-chain attestation with:
- explicit circuit artifact policy (`dev` vs `production`)
- signer hardening (keystore-first, private-key-env fallback)
- public input schema lock (v1 ordering)
- deploy/e2e artifact chain with verifier script

## Circuit Policy

`agenthalo onchain config --circuit-policy dev|production`

- `dev`:
  - deterministic key generation when missing
  - writes `~/.agenthalo/circuit/pk.bin`, `vk.bin`, `metadata.json`
- `production`:
  - fail-closed when any artifact is missing
  - fail-closed on metadata schema mismatch or key-hash mismatch

Metadata file: `~/.agenthalo/circuit/metadata.json`

Fields:
- `schema_version`
- `setup_mode`
- `created_at`
- `max_events`
- `public_input_schema_version`
- `pk_sha256`
- `vk_sha256`

## Signer Modes

`agenthalo onchain config --signer-mode private_key_env|keystore`

Config fields:
- `private_key_env`
- `keystore_path`
- `keystore_password_file`

Behavior:
- if keystore path + password file are present, keystore is used
- otherwise private-key env fallback is used
- chain mismatch fails closed
- verify calls enforce gas cap (`<= 500000`)
- nonce/replacement errors use bounded retry

## Public Input Schema v1

Ordering is fixed:
- `[0] MERKLE_LO`
- `[1] MERKLE_HI`
- `[2] DIGEST_LO`
- `[3] DIGEST_HI`
- `[4] EVENT_COUNT`

Rust source of truth:
- `src/halo/public_input_schema.rs`

Solidity contract docs:
- `contracts/TrustVerifier.sol`

## Deployment / E2E

Deploy:

```bash
cd contracts
./scripts/deploy_agenthalo_trust_base_sepolia.sh
```

E2E:

```bash
cd contracts
./scripts/e2e_agenthalo_attestation_base_sepolia.sh
```

Artifact verification:

```bash
cd contracts
./scripts/verify_agenthalo_phase5_artifacts.py \
  --deployment <deploy.json> \
  --e2e <e2e.json>
```

Strict live verification:

```bash
./scripts/verify_agenthalo_phase5_artifacts.py \
  --deployment <deploy.json> \
  --e2e <e2e.json> \
  --require-live
```

## Evidence Chain

- deploy artifact includes `script_sha256`
- e2e artifact includes:
  - `script_sha256`
  - `deployment_evidence_sha256` (hash of deployment artifact)
- verifier checks schema, formatting, and linkage invariants
