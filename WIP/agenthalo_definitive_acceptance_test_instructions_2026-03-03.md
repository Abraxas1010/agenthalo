# Definitive Full Acceptance Test — AgentHALO / NucleusDB

**Version:** 2.0 (supersedes `agenthalo_full_acceptance_test_instructions_2026-03-02.md`)
**Target commit:** current HEAD of `origin/master`
**Test baseline:** 686+ tests passing, 241 Lean theorems/lemmas, zero sorry/admit

---

## Mission

You are an AI agent performing a **comprehensive acceptance test** of the AgentHALO/NucleusDB system. You will:

1. **Clean-slate build** from source — verify all binaries compile
2. **Run the full automated test suite** and record exact counts
3. **Launch THREE independent instances** on ports 3100/3200/3300
4. **Systematically exercise every CLI subcommand, API endpoint, and MCP tool** from all three instances
5. **Verify all cryptographic primitives** end-to-end: ZK proofs, SNARK receipts, ML-DSA signatures, AES-256-GCM vault encryption, DIDComm AEAD, identity ledger hash chains, NucleusDB seal chains, vector commitment proofs (IPA/KZG), PUF fingerprints, CT6962 transparency logs
6. **Verify the Lean formal proof surface**: `lake build --wfail`, zero sorry/admit, theorem count, proof-gate enforcement, and correspondence between Lean specs and Rust implementations
7. **Test Nym privacy transport**: fail-open/fail-closed modes, privacy classification, SOCKS5 detection
8. **Test the sovereign comms stack end-to-end**: container mesh registration, DIDComm encrypted envelopes (all 7 message types), cross-instance MCP tool calls, capability delegation triangle
9. **Inspect ALL logs, databases, and artifact files** post-test: trace DBs, genesis seeds, vault files, identity ledgers, mesh peer registries, ZK receipts, seal chains
10. **Hunt for improvements**: UX friction, error message quality, missing validation, documentation drift, hardening gaps
11. **Deliver a structured report** with pass/fail per phase, reproduction steps for every failure, and a prioritized improvement backlog

---

## Critical Rules

1. **No skipping.** Every phase must be executed. If a phase fails, record the exact error and continue.
2. **Exact commands.** Record every command and its output (key lines if verbose).
3. **No assumptions.** Test everything explicitly — never assume it works.
4. **All three instances.** Where the instructions say "all three," test all three. Data isolation is a security property.
5. **Save all evidence.** Redirect all output to `/tmp/acceptance/` with descriptive filenames.
6. **Report improvements.** Every phase should note suggested improvements, not just pass/fail.
7. **Clean up.** Kill all processes at the end.
8. **Use the correct workflow ordering.** Genesis harvest creates the PQ wallet — do NOT run `keygen --pq` after genesis (it will orphan the genesis seed). The correct order is: genesis harvest → crypto create-password → unlock → use.
9. **Set mesh env vars.** All instances need `NUCLEUSDB_MESH_AGENT_ID`, `NUCLEUSDB_MESH_PORT`, and `NUCLEUSDB_MESH_REGISTRY` for mesh features to work.
10. **Use `AGENTHALO_ALLOW_GENERIC=1`** for the `agenthalo run` wrapper in QA (free-tier guard).

---

## Phase 0: Clean Slate + Build Gate

```bash
cd /home/abraxas/Work/nucleusdb
mkdir -p /tmp/acceptance

# Kill any running instances
pkill -f 'agenthalo' || true
pkill -f 'nucleusdb' || true
sleep 2

# Delete ALL local data — fresh start
rm -rf ~/.agenthalo
rm -rf ~/.nucleusdb
rm -rf /tmp/agenthalo_*
rm -rf /tmp/nucleusdb_*
rm -rf /tmp/acceptance/*

# Build release
cargo build --release 2>&1 | tee /tmp/acceptance/build_output.txt
echo "Build exit: $?"

# Verify all 6 binaries
for bin in agenthalo agenthalo-mcp-server nucleusdb nucleusdb-server nucleusdb-mcp nucleusdb-tui; do
  ls -la target/release/$bin 2>/dev/null && echo "OK: $bin" || echo "MISSING: $bin"
done | tee /tmp/acceptance/binary_check.txt

# Run full test suite
cargo test --release 2>&1 | tee /tmp/acceptance/test_output.txt
echo "Test exit: $?"

# Parse and record test counts
grep 'test result:' /tmp/acceptance/test_output.txt | tee /tmp/acceptance/test_summary.txt
echo "Total passed:"
grep 'test result:' /tmp/acceptance/test_output.txt | grep -oP '\d+ passed' | awk -F' ' '{s+=$1} END {print s}'
```

**Gate:** All 6 binaries exist. All tests pass. Record exact test count (baseline: 686+).

---

## Phase 1: Launch Three Instances

```bash
# Instance directories
export AGENTHALO_HOME_A="/tmp/agenthalo_qa_instance_a"
export AGENTHALO_HOME_B="/tmp/agenthalo_qa_instance_b"
export AGENTHALO_HOME_C="/tmp/agenthalo_qa_instance_c"
mkdir -p "$AGENTHALO_HOME_A" "$AGENTHALO_HOME_B" "$AGENTHALO_HOME_C"

# Shared mesh registry (all instances must use the same file)
export MESH_REGISTRY="/tmp/halo_mesh_peers.json"

# Launch Instance A (port 3100)
AGENTHALO_HOME="$AGENTHALO_HOME_A" \
  NUCLEUSDB_MESH_AGENT_ID="instance-a" \
  NUCLEUSDB_MESH_PORT=3100 \
  NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo dashboard --port 3100 --no-open &
PID_A=$!

# Launch Instance B (port 3200)
AGENTHALO_HOME="$AGENTHALO_HOME_B" \
  NUCLEUSDB_MESH_AGENT_ID="instance-b" \
  NUCLEUSDB_MESH_PORT=3200 \
  NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo dashboard --port 3200 --no-open &
PID_B=$!

# Launch Instance C (port 3300)
AGENTHALO_HOME="$AGENTHALO_HOME_C" \
  NUCLEUSDB_MESH_AGENT_ID="instance-c" \
  NUCLEUSDB_MESH_PORT=3300 \
  NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo dashboard --port 3300 --no-open &
PID_C=$!

sleep 3

# Verify all three respond
for port in 3100 3200 3300; do
  curl -sf http://127.0.0.1:$port/api/status > /tmp/acceptance/status_${port}.json && echo "[$port OK]" || echo "[$port FAIL]"
done
```

**Gate:** All three instances respond with valid JSON on `/api/status`.

---

## Phase 2: Genesis Ceremony (All Three Instances)

**CRITICAL ORDERING:** Genesis harvest auto-creates the PQ wallet. Do NOT run `keygen --pq` later — it will orphan the seed.

```bash
# Genesis on all three
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "=== Genesis Instance $inst ==="

  # Harvest (auto-creates PQ wallet)
  NYM_FAIL_OPEN=1 AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo genesis harvest 2>&1 \
    | tee /tmp/acceptance/genesis_harvest_${inst}.txt

  # Status (JSON)
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo genesis status --json 2>&1 \
    | tee /tmp/acceptance/genesis_status_${inst}.json
done

# Verify: all three seeds are DIFFERENT
python3 -c "
import json, sys
seeds = {}
for inst in ['A', 'B', 'C']:
    try:
        with open(f'/tmp/acceptance/genesis_status_{inst}.json') as f:
            data = json.load(f)
        seeds[inst] = data.get('seed_hash', data.get('entropy_hash', 'UNKNOWN'))
    except Exception as e:
        seeds[inst] = f'ERROR: {e}'
print('Genesis seed uniqueness:')
for k, v in seeds.items():
    print(f'  {k}: {v}')
unique = len(set(v for v in seeds.values() if not v.startswith('ERROR')))
print(f'Unique seeds: {unique}/3 — {\"PASS\" if unique == 3 else \"FAIL\"}')" \
  | tee /tmp/acceptance/genesis_uniqueness.txt

# Dashboard genesis status
for port in 3100 3200 3300; do
  curl -s http://127.0.0.1:$port/api/genesis/status \
    | python3 -m json.tool > /tmp/acceptance/genesis_dashboard_${port}.json 2>&1
done
```

**Verify:**
- Each genesis seed file exists and is encrypted
- Entropy sources recorded (list count + which sources)
- SHA-256 is stable (query twice, must match)
- All three instances have DIFFERENT seed hashes
- Dashboard genesis endpoint returns valid JSON

---

## Phase 3: Crypto Lifecycle (All Three Instances)

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  eval PORT_VAR=$((3000 + ($(echo $inst | tr 'ABC' '123') * 100)))
  echo "=== Crypto Instance $inst (port $PORT_VAR) ==="

  # Create password (non-interactive via stdin)
  echo "testpass-${inst}-2026" | AGENTHALO_HOME="$HOME_VAR" \
    AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:${PORT_VAR}/api \
    target/release/agenthalo crypto create-password 2>&1 \
    | tee /tmp/acceptance/crypto_create_${inst}.txt

  # Status should show unlocked
  AGENTHALO_HOME="$HOME_VAR" \
    AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:${PORT_VAR}/api \
    target/release/agenthalo crypto status 2>&1 \
    | tee /tmp/acceptance/crypto_status_unlocked_${inst}.txt

  # Lock
  AGENTHALO_HOME="$HOME_VAR" \
    AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:${PORT_VAR}/api \
    target/release/agenthalo crypto lock 2>&1 \
    | tee /tmp/acceptance/crypto_lock_${inst}.txt

  # Status should show locked
  AGENTHALO_HOME="$HOME_VAR" \
    AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:${PORT_VAR}/api \
    target/release/agenthalo crypto status 2>&1 \
    | tee /tmp/acceptance/crypto_status_locked_${inst}.txt

  # Unlock with wrong password (must fail)
  echo "wrong-password" | AGENTHALO_HOME="$HOME_VAR" \
    AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:${PORT_VAR}/api \
    target/release/agenthalo crypto unlock 2>&1 \
    | tee /tmp/acceptance/crypto_unlock_wrong_${inst}.txt

  # Unlock with correct password
  echo "testpass-${inst}-2026" | AGENTHALO_HOME="$HOME_VAR" \
    AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:${PORT_VAR}/api \
    target/release/agenthalo crypto unlock 2>&1 \
    | tee /tmp/acceptance/crypto_unlock_correct_${inst}.txt
done
```

### 3.2 API Key Auth

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  KEY="test-api-key-instance-$(echo $inst | tr 'A-C' 'a-c')"
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo config set-key "$KEY" 2>&1 \
    | tee /tmp/acceptance/config_setkey_${inst}.txt
done

# Verify auth works
curl -s -H "Authorization: Bearer test-api-key-instance-a" http://127.0.0.1:3100/api/vault/keys \
  > /tmp/acceptance/auth_valid.json 2>&1

# Verify wrong key rejected
curl -s -w "\nHTTP_CODE:%{http_code}" -H "Authorization: Bearer wrong-key" http://127.0.0.1:3100/api/vault/keys \
  > /tmp/acceptance/auth_invalid.txt 2>&1

# Verify cross-instance isolation: A's key must NOT work on B
curl -s -w "\nHTTP_CODE:%{http_code}" -H "Authorization: Bearer test-api-key-instance-a" http://127.0.0.1:3200/api/vault/keys \
  > /tmp/acceptance/auth_cross_instance.txt 2>&1
```

**Verify:**
- Password create/lock/unlock/wrong-password cycle works on all three
- API key auth works with correct key
- Wrong key returns 401/403
- Cross-instance keys are isolated

---

## Phase 4: Vault CRUD + Encryption

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "=== Vault Instance $inst ==="

  # Set a key
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo vault set openrouter "sk-or-test-${inst}" 2>&1 \
    | tee /tmp/acceptance/vault_set_${inst}.txt

  # List — must show openrouter
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo vault list 2>&1 \
    | tee /tmp/acceptance/vault_list_${inst}.txt

  # Test — must show MASKED key (never full)
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo vault test openrouter 2>&1 \
    | tee /tmp/acceptance/vault_test_${inst}.txt

  # Delete
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo vault delete openrouter 2>&1 \
    | tee /tmp/acceptance/vault_delete_${inst}.txt

  # List again — openrouter must be gone
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo vault list 2>&1 \
    | tee /tmp/acceptance/vault_list_after_delete_${inst}.txt
done

# Verify vault file is encrypted on disk (not plaintext)
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "=== Vault encryption check $inst ==="
  file "$HOME_VAR/vault.enc" 2>/dev/null || echo "vault.enc not found (may use different filename)"
  hexdump -C "$HOME_VAR/vault.enc" 2>/dev/null | head -3
done | tee /tmp/acceptance/vault_encryption_check.txt
```

### 4.2 Dashboard Vault API

```bash
KEY_A="test-api-key-instance-a"

# Store via API
curl -s -X POST -H "Authorization: Bearer $KEY_A" \
  -H "Content-Type: application/json" \
  -d '{"key":"sk-or-test-via-api"}' \
  http://127.0.0.1:3100/api/vault/keys/openrouter > /tmp/acceptance/vault_api_set.json 2>&1

# List via API
curl -s -H "Authorization: Bearer $KEY_A" http://127.0.0.1:3100/api/vault/keys \
  > /tmp/acceptance/vault_api_list.json 2>&1

# Test via API
curl -s -X POST -H "Authorization: Bearer $KEY_A" \
  http://127.0.0.1:3100/api/vault/keys/openrouter/test > /tmp/acceptance/vault_api_test.json 2>&1

# Delete via API
curl -s -X DELETE -H "Authorization: Bearer $KEY_A" \
  http://127.0.0.1:3100/api/vault/keys/openrouter > /tmp/acceptance/vault_api_delete.json 2>&1
```

---

## Phase 5: Agent Recording (Wrap + Trace)

```bash
# Record commands on Instance A
AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_ALLOW_GENERIC=1 \
  target/release/agenthalo run /bin/echo "hello from instance A" 2>&1 \
  | tee /tmp/acceptance/run_a1.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_ALLOW_GENERIC=1 \
  target/release/agenthalo run /bin/ls -la /tmp/ 2>&1 \
  | tee /tmp/acceptance/run_a2.txt

# Record on Instance B
AGENTHALO_HOME="$AGENTHALO_HOME_B" AGENTHALO_ALLOW_GENERIC=1 \
  target/release/agenthalo run /bin/echo "hello from instance B" 2>&1 \
  | tee /tmp/acceptance/run_b1.txt

# List traces
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo traces --json \
  > /tmp/acceptance/traces_a.json 2>&1
AGENTHALO_HOME="$AGENTHALO_HOME_B" target/release/agenthalo traces --json \
  > /tmp/acceptance/traces_b.json 2>&1

# Get session ID for later use
SESSION_A=$(python3 -c "import json; d=json.load(open('/tmp/acceptance/traces_a.json')); print(d[0]['session_id'] if d else '')" 2>/dev/null)
echo "Session A: $SESSION_A" | tee /tmp/acceptance/session_a_id.txt

# Verify data isolation: A traces NOT visible from B
python3 -c "
import json
a = json.load(open('/tmp/acceptance/traces_a.json'))
b = json.load(open('/tmp/acceptance/traces_b.json'))
a_ids = {s['session_id'] for s in a}
b_ids = {s['session_id'] for s in b}
overlap = a_ids & b_ids
print(f'A sessions: {len(a_ids)}, B sessions: {len(b_ids)}')
print(f'Overlap: {len(overlap)} — {\"FAIL\" if overlap else \"PASS\"}')" \
  | tee /tmp/acceptance/trace_isolation.txt

# Export
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo export "$SESSION_A" --out /tmp/acceptance/export_a.json 2>&1
python3 -m json.tool /tmp/acceptance/export_a.json > /dev/null && echo "VALID JSON" || echo "INVALID JSON"
```

---

## Phase 6: Immutable Log Integrity + Seal Chains

### 6.1 Trace Store Immutability

```bash
# Write 5 sessions
for i in 1 2 3 4 5; do
  AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_ALLOW_GENERIC=1 \
    target/release/agenthalo run /bin/echo "immutability test $i" 2>&1 > /dev/null
done

# Verify session count increased
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo traces --json > /tmp/acceptance/traces_after_immutability.json
python3 -c "
import json
sessions = json.load(open('/tmp/acceptance/traces_after_immutability.json'))
print(f'Total sessions: {len(sessions)}')
for s in sessions[:5]:
    print(f'  {s[\"session_id\"]} events={s.get(\"event_count\",\"?\")}')" \
  | tee /tmp/acceptance/immutability_count.txt

# Backup + tamper test
cp "$AGENTHALO_HOME_A/traces.redb" /tmp/acceptance/traces_backup.redb 2>/dev/null || \
  echo "WARN: traces.redb not found at expected path"
```

### 6.2 NucleusDB Monotone Seal Chain

```bash
# Create a DB, write data, verify seal chain
target/release/nucleusdb --db /tmp/acceptance/qa_seal_chain.db <<'SQL'
SET key1 = 'value1';
SET key2 = 'value2';
COMMIT;
SET key3 = 'value3';
COMMIT;
SQL
echo "Seal chain exit: $?" | tee /tmp/acceptance/seal_chain_result.txt

# Query metadata
target/release/nucleusdb --db /tmp/acceptance/qa_seal_chain.db <<'SQL' > /tmp/acceptance/seal_chain_meta.txt 2>&1
SELECT * FROM _meta;
SQL
```

### 6.3 Vector Commitment Proofs (IPA)

```bash
target/release/nucleusdb --db /tmp/acceptance/qa_vc_ipa.db --backend ipa <<'SQL' > /tmp/acceptance/vc_ipa.txt 2>&1
SET a = 1;
SET b = 2;
SET c = 3;
COMMIT;
PROOF a;
SQL
echo "IPA proof exit: $?"
```

### 6.4 CT6962 Transparency Log (Merkle Inclusion + Consistency)

```bash
# The transparency module implements RFC 6962 certificate transparency
# Verify via unit tests
cargo test --release ct6962 -- --nocapture 2>&1 | tee /tmp/acceptance/ct6962_tests.txt
```

---

## Phase 7: Attestation, Trust, & Post-Quantum Signatures

### 7.1 PQ Wallet (Already Created by Genesis)

```bash
# DO NOT run keygen --pq here — genesis already created the wallet
# Just verify it exists
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "=== PQ Wallet $inst ==="
  ls -la "$HOME_VAR/pq_wallet.json" 2>/dev/null || echo "MISSING: pq_wallet.json"
done | tee /tmp/acceptance/pq_wallet_check.txt

# Verify all three have DIFFERENT public keys
python3 -c "
import json
for inst in ['A', 'B', 'C']:
    path = f'/tmp/agenthalo_qa_instance_{inst.lower()}/pq_wallet.json'
    try:
        with open(path) as f:
            w = json.load(f)
        pk = w.get('public_key', w.get('pub_key', 'UNKNOWN'))[:32]
        print(f'  {inst}: {pk}...')
    except Exception as e:
        print(f'  {inst}: ERROR {e}')" \
  | tee /tmp/acceptance/pq_wallet_uniqueness.txt
```

### 7.2 Attestation (Non-Anonymous + Anonymous)

```bash
# Non-anonymous attestation
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo attest --session "$SESSION_A" 2>&1 \
  | tee /tmp/acceptance/attest_nonanon.json

# Anonymous attestation
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo attest --session "$SESSION_A" --anonymous 2>&1 \
  | tee /tmp/acceptance/attest_anon.json

# Verify both are valid JSON with expected fields
python3 -c "
import json
for label, path in [('nonanon', '/tmp/acceptance/attest_nonanon.json'), ('anon', '/tmp/acceptance/attest_anon.json')]:
    try:
        with open(path) as f:
            att = json.load(f)
        keys = list(att.keys())
        print(f'{label}: keys={keys}')
        for k in ['merkle_root', 'digest', 'signature']:
            print(f'  {k}: {\"present\" if k in att else \"MISSING\"}')
    except Exception as e:
        print(f'{label}: ERROR {e}')" \
  | tee /tmp/acceptance/attest_field_check.txt
```

### 7.3 Trust Score

```bash
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo trust query --session "$SESSION_A" 2>&1 \
  | tee /tmp/acceptance/trust_query.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo trust score 2>&1 \
  | tee /tmp/acceptance/trust_score.txt
```

### 7.4 ML-DSA Post-Quantum Signatures

```bash
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo sign --pq --message "test message from instance A" 2>&1 \
  | tee /tmp/acceptance/sign_pq_a.txt

# Verify: output contains ML-DSA-65 signature (hex string or base64)
grep -i 'signature\|ML-DSA\|ml_dsa' /tmp/acceptance/sign_pq_a.txt \
  | tee /tmp/acceptance/sign_pq_verify.txt
```

---

## Phase 7.5: ZK Credential Round-Trip (SNARK Mechanism)

```bash
# Step 1: Create a test grant (capability token)
GRANT_OUTPUT=$(AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access grant \
  "did:key:qa-agent-zk" "traces" --modes "read" --ttl 3600 2>&1)
echo "$GRANT_OUTPUT" | tee /tmp/acceptance/zk_grant.txt

# Parse grant token ID
GRANT_TOKEN_ID=$(echo "$GRANT_OUTPUT" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('token_id',''))" 2>/dev/null || echo "PARSE_FAILED")
echo "Grant token ID: $GRANT_TOKEN_ID" | tee /tmp/acceptance/zk_grant_id.txt

# Step 2: ZK credential prove
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk credential prove \
  --grant-id "$GRANT_TOKEN_ID" \
  --resource "traces" --action read \
  --out /tmp/acceptance/zk_credential_proof.json 2>&1 \
  | tee /tmp/acceptance/zk_credential_prove.txt

# Step 3: ZK credential verify — MUST succeed
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk credential verify \
  --proof /tmp/acceptance/zk_credential_proof.json 2>&1 \
  | tee /tmp/acceptance/zk_credential_verify.txt
echo "Credential verify exit: $?"

# Step 4: Anonymous membership prove + verify
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo identity status --json > /tmp/acceptance/identity_for_registry.json 2>&1
python3 -c "
import json
with open('/tmp/acceptance/identity_for_registry.json') as f:
    status = json.load(f)
registry = {'members': [status.get('did', 'unknown')]}
with open('/tmp/acceptance/zk_registry.json', 'w') as f:
    json.dump(registry, f)
print('Registry:', registry)"

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk anonymous prove \
  --grant-id "$GRANT_TOKEN_ID" \
  --resource "traces" --action read \
  --registry /tmp/acceptance/zk_registry.json \
  --out /tmp/acceptance/zk_anon_proof.json 2>&1 \
  | tee /tmp/acceptance/zk_anon_prove.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk anonymous verify \
  --proof /tmp/acceptance/zk_anon_proof.json \
  --registry /tmp/acceptance/zk_registry.json 2>&1 \
  | tee /tmp/acceptance/zk_anon_verify.txt

# Step 5: Cross-instance isolation — proof from A must NOT verify on B's keyset
AGENTHALO_HOME="$AGENTHALO_HOME_B" target/release/agenthalo zk credential verify \
  --proof /tmp/acceptance/zk_credential_proof.json 2>&1 \
  | tee /tmp/acceptance/zk_cross_instance_verify.txt
echo "Cross-instance verify exit (should be non-zero): $?"
```

---

## Phase 7.6: ZK Verifiable Computation (All 4 Builtin Guests)

```bash
# --- Range Proof ---
python3 -c "
import json, struct
json.dump([0, 100], open('/tmp/acceptance/zk_range_input.json','w'))
with open('/tmp/acceptance/zk_range_private.bin','wb') as f:
    f.write(struct.pack('<Q', 42))
"

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute prove \
  --guest range_proof \
  --input /tmp/acceptance/zk_range_input.json \
  --private /tmp/acceptance/zk_range_private.bin \
  --compute-id "qa-range-test" \
  --out /tmp/acceptance/zk_compute_range.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_range_prove.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute verify \
  --receipt /tmp/acceptance/zk_compute_range.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_range_verify.txt

# --- Set Membership ---
python3 -c "
import json
json.dump({'set': [10, 20, 30, 40, 50], 'element': 30}, open('/tmp/acceptance/zk_set_input.json','w'))
"

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute prove \
  --guest set_membership \
  --input /tmp/acceptance/zk_set_input.json \
  --compute-id "qa-set-member-test" \
  --out /tmp/acceptance/zk_compute_set.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_set_prove.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute verify \
  --receipt /tmp/acceptance/zk_compute_set.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_set_verify.txt

# --- Secure Aggregation ---
python3 -c "
import json
json.dump({'values': [100, 200, 300], 'policy': 'sum'}, open('/tmp/acceptance/zk_agg_input.json','w'))
"

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute prove \
  --guest secure_aggregation \
  --input /tmp/acceptance/zk_agg_input.json \
  --compute-id "qa-agg-test" \
  --out /tmp/acceptance/zk_compute_agg.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_agg_prove.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute verify \
  --receipt /tmp/acceptance/zk_compute_agg.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_agg_verify.txt

# --- Algorithm Compliance ---
python3 -c "
import json
json.dump({'algorithm': 'sha256', 'input_hex': '48656c6c6f', 'expected_output_hex': '185f8db32271fe25f561a6fc938b2e264306ec304eda518007d1764826381969'}, open('/tmp/acceptance/zk_algo_input.json','w'))
"

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute prove \
  --guest algorithm_compliance \
  --input /tmp/acceptance/zk_algo_input.json \
  --compute-id "qa-algo-test" \
  --out /tmp/acceptance/zk_compute_algo.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_algo_prove.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute verify \
  --receipt /tmp/acceptance/zk_compute_algo.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_algo_verify.txt

# --- Tamper Detection ---
python3 -c "
import json
try:
    with open('/tmp/acceptance/zk_compute_range.json') as f:
        receipt = json.load(f)
    # Tamper with compute_id
    if 'receipt' in receipt and 'envelope' in receipt['receipt']:
        receipt['receipt']['envelope']['compute_id'] = 'tampered-id'
    elif 'compute_id' in receipt:
        receipt['compute_id'] = 'tampered-id'
    with open('/tmp/acceptance/zk_compute_tampered.json','w') as f:
        json.dump(receipt, f)
    print('Tampered receipt created')
except Exception as e:
    print(f'Tamper creation failed: {e}')
"

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo zk compute verify \
  --receipt /tmp/acceptance/zk_compute_tampered.json 2>&1 \
  | tee /tmp/acceptance/zk_compute_tampered_verify.txt
echo "Tampered verify exit (MUST be non-zero): $?"

# Verify image IDs match deterministic derivation
cargo test --release image_ids -- --nocapture 2>&1 | tee /tmp/acceptance/zk_image_id_tests.txt
```

---

## Phase 7B: Lean Proof Gate Enforcement

```bash
# 7B.1 Status (disabled by default)
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo proof-gate status 2>&1 \
  | tee /tmp/acceptance/proof_gate_status.txt

# 7B.2 Submit + Verify certificate
cat > /tmp/acceptance/test_cert.lean4export << 'EOF'
#THM HeytingLean.NucleusDB.Core.replay_preserves
#AX propext
EOF

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo proof-gate submit /tmp/acceptance/test_cert.lean4export 2>&1 \
  | tee /tmp/acceptance/proof_gate_submit.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo proof-gate verify /tmp/acceptance/test_cert.lean4export 2>&1 \
  | tee /tmp/acceptance/proof_gate_verify.txt

# 7B.3 Enforcement — gate blocks without cert
cat > /tmp/acceptance/proof_gate_config.json << GATE
{
  "certificate_dir": "${AGENTHALO_HOME_A}/proof_certificates",
  "enabled": true,
  "requirements": {
    "nucleusdb_execute_sql": [{
      "tool_name": "nucleusdb_execute_sql",
      "required_theorem": "HeytingLean.NucleusDB.Core.replay_preserves",
      "description": "Replay must preserve state invariants",
      "enforced": true
    }]
  }
}
GATE

export AGENTHALO_PROOF_GATE_CONFIG=/tmp/acceptance/proof_gate_config.json

# With cert present — gate PASSES
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo proof-gate status 2>&1 \
  | tee /tmp/acceptance/proof_gate_with_cert.txt

# Remove cert — gate FAILS
rm -f "${AGENTHALO_HOME_A}/proof_certificates/test_cert.lean4export"
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo proof-gate status 2>&1 \
  | tee /tmp/acceptance/proof_gate_without_cert.txt

unset AGENTHALO_PROOF_GATE_CONFIG
```

---

## Phase 7C: Lean Formal Spec Integrity (241 Theorems)

```bash
cd /home/abraxas/Work/nucleusdb

# Build the Lean library (incremental — do NOT lake clean)
(cd lean && lake build --wfail 2>&1 | tail -20) | tee /tmp/acceptance/lean_build.txt
echo "Lean build exit: $?"

# Zero sorry/admit check
grep -rn 'sorry\|admit' lean/NucleusDB/ > /tmp/acceptance/lean_sorry_scan.txt 2>&1
SORRY_COUNT=$(wc -l < /tmp/acceptance/lean_sorry_scan.txt)
echo "Sorry/admit count: $SORRY_COUNT — $([ $SORRY_COUNT -eq 0 ] && echo PASS || echo FAIL)" \
  | tee -a /tmp/acceptance/lean_sorry_scan.txt

# Count ALL theorems and lemmas
THEOREM_COUNT=$(grep -rn '^theorem\|^lemma' lean/NucleusDB/ | wc -l)
echo "Total theorems/lemmas: $THEOREM_COUNT (baseline: 241)" \
  | tee /tmp/acceptance/lean_theorem_count.txt

# Verify key theorem domains exist
echo "--- Core theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Core/ | wc -l
echo "--- Comms theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Comms/ | wc -l
echo "--- Identity theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Identity/ | wc -l
echo "--- Genesis theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Genesis/ | wc -l
echo "--- Security theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Security/ | wc -l
echo "--- PaymentChannels theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/PaymentChannels/ | wc -l
echo "--- ZK theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Comms/ZK/ | wc -l
echo "--- Transparency theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Transparency/ | wc -l
echo "--- Sheaf theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Sheaf/ | wc -l
echo "--- Adversarial theorems ---"
grep -rn '^theorem\|^lemma' lean/NucleusDB/Adversarial/ | wc -l

# List ALL theorem names for audit
grep -rn '^theorem\|^lemma' lean/NucleusDB/ | sed 's/.*theorem /theorem /; s/.*lemma /lemma /' | sort \
  | tee /tmp/acceptance/lean_theorem_list.txt
```

**Verify:**
- `lake build --wfail` exits 0 (no warnings-as-errors)
- Zero sorry/admit in `lean/NucleusDB/`
- Theorem count >= 241 (regression check)
- Key domains have theorems: Core, Comms (mesh, DIDComm, auth chain, ZK), Identity, Genesis, Security, PaymentChannels, Transparency, Sheaf, Adversarial

---

## Phase 8: Identity System

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "=== Identity $inst ==="

  # Status
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity status --json 2>&1 \
    | tee /tmp/acceptance/identity_status_${inst}.json

  # Profile set/get
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity profile set "QA Tester $inst" 2>&1 \
    | tee /tmp/acceptance/identity_profile_set_${inst}.txt
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity profile get 2>&1 \
    | tee /tmp/acceptance/identity_profile_get_${inst}.txt

  # Device fingerprint
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity device scan 2>&1 \
    | tee /tmp/acceptance/identity_device_${inst}.txt

  # Network identity
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity network probe 2>&1 \
    | tee /tmp/acceptance/identity_network_${inst}.txt

  # Anonymous mode toggle
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity anonymous set true 2>&1 \
    | tee /tmp/acceptance/identity_anon_on_${inst}.txt
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo identity anonymous set false 2>&1 \
    | tee /tmp/acceptance/identity_anon_off_${inst}.txt
done

# Verify identity ledger hash chain integrity
python3 -c "
import json
for inst in ['A', 'B', 'C']:
    try:
        with open(f'/tmp/acceptance/identity_status_{inst}.json') as f:
            status = json.load(f)
        ledger = status.get('ledger_entries', status.get('entries', []))
        chain = status.get('chain_valid', 'UNKNOWN')
        did = status.get('did', 'UNKNOWN')
        print(f'{inst}: DID={did[:40]}... entries={len(ledger) if isinstance(ledger, list) else ledger} chain={chain}')
    except Exception as e:
        print(f'{inst}: ERROR {e}')" \
  | tee /tmp/acceptance/identity_chain_check.txt
```

---

## Phase 9: AgentAddress & Wallet

```bash
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo agentaddress status 2>&1 \
  | tee /tmp/acceptance/agentaddress_status.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo agentaddress chains 2>&1 \
  | tee /tmp/acceptance/agentaddress_chains.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo agentaddress generate --source external --persist-public true 2>&1 \
  | tee /tmp/acceptance/agentaddress_generate.txt

# Wallet
AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:3100/api \
  target/release/agenthalo wallet status 2>&1 \
  | tee /tmp/acceptance/wallet_status.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:3100/api \
  target/release/agenthalo wallet create 2>&1 \
  | tee /tmp/acceptance/wallet_create.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_DASHBOARD_API_BASE=http://127.0.0.1:3100/api \
  target/release/agenthalo wallet accounts 2>&1 \
  | tee /tmp/acceptance/wallet_accounts.txt
```

---

## Phase 10: Communication Channels + Nym Privacy

### 10.1 Nym/SOCKS5 Status & Privacy Classification

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "=== Nym $inst ==="

  # Nym status
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo nym status 2>&1 \
    | tee /tmp/acceptance/nym_status_${inst}.txt

  # Privacy classification — local vs external routing
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo privacy classify https://api.openrouter.ai 2>&1 \
    | tee /tmp/acceptance/privacy_external_${inst}.txt
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo privacy classify http://localhost:3100 2>&1 \
    | tee /tmp/acceptance/privacy_local_${inst}.txt
done

# Fail-closed test: with NYM_FAIL_OPEN=0, external calls should be blocked
NYM_FAIL_OPEN=0 AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo privacy classify https://api.openrouter.ai 2>&1 \
  | tee /tmp/acceptance/nym_fail_closed.txt
```

### 10.2 Comms Stack Status

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo comms status 2>&1 \
    | tee /tmp/acceptance/comms_status_${inst}.txt
done
```

---

## Phase 10B: Container Mesh Network

### 10B.1 Mesh Status (All Three)

```bash
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  AGENT_ID="instance-$(echo $inst | tr 'A-C' 'a-c')"
  AGENTHALO_HOME="$HOME_VAR" NUCLEUSDB_MESH_AGENT_ID="$AGENT_ID" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
    target/release/agenthalo mesh status 2>&1 \
    | tee /tmp/acceptance/mesh_status_${inst}.txt
done

# Inspect the shared peer registry file
cat "$MESH_REGISTRY" 2>/dev/null | python3 -m json.tool > /tmp/acceptance/mesh_registry_contents.json 2>&1
```

### 10B.2 Mesh Simulation Tests (15 Tests)

```bash
cargo test --release mesh_simulation -- --nocapture 2>&1 | tee /tmp/acceptance/mesh_sim_tests.txt
grep -E 'test sim_|test result:' /tmp/acceptance/mesh_sim_tests.txt | tee /tmp/acceptance/mesh_sim_summary.txt
```

### 10B.3 Mesh Ping (All 6 Directed Pairs)

```bash
for src_inst in A B C; do
  for dst_inst in A B C; do
    [ "$src_inst" = "$dst_inst" ] && continue
    SRC_ID="instance-$(echo $src_inst | tr 'A-C' 'a-c')"
    DST_ID="instance-$(echo $dst_inst | tr 'A-C' 'a-c')"
    eval HOME_VAR=\$AGENTHALO_HOME_$src_inst
    AGENTHALO_HOME="$HOME_VAR" NUCLEUSDB_MESH_AGENT_ID="$SRC_ID" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
      target/release/agenthalo mesh ping "$DST_ID" 2>&1 \
      | tee /tmp/acceptance/mesh_ping_${src_inst}_to_${dst_inst}.txt
  done
done
```

### 10B.4 Mesh Remote Tool Call

```bash
AGENTHALO_HOME="$AGENTHALO_HOME_A" NUCLEUSDB_MESH_AGENT_ID="instance-a" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo mesh call instance-b halo_status 2>&1 \
  | tee /tmp/acceptance/mesh_call_a_to_b.txt

AGENTHALO_HOME="$AGENTHALO_HOME_B" NUCLEUSDB_MESH_AGENT_ID="instance-b" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo mesh call instance-c crypto_status --args '{}' 2>&1 \
  | tee /tmp/acceptance/mesh_call_b_to_c.txt
```

### 10B.5 Mesh Capability Grant (Triangular)

```bash
# A grants B: nucleusdb_* read+execute
AGENTHALO_HOME="$AGENTHALO_HOME_A" NUCLEUSDB_MESH_AGENT_ID="instance-a" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo mesh grant instance-b \
  --patterns "nucleusdb_*" --modes "read,execute" --duration 3600 2>&1 \
  | tee /tmp/acceptance/mesh_grant_a_to_b.txt

# B grants C: halo_* read
AGENTHALO_HOME="$AGENTHALO_HOME_B" NUCLEUSDB_MESH_AGENT_ID="instance-b" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo mesh grant instance-c \
  --patterns "halo_*" --modes "read" --duration 3600 2>&1 \
  | tee /tmp/acceptance/mesh_grant_b_to_c.txt

# C grants A: crypto_status read
AGENTHALO_HOME="$AGENTHALO_HOME_C" NUCLEUSDB_MESH_AGENT_ID="instance-c" NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo mesh grant instance-a \
  --patterns "crypto_status" --modes "read" --duration 3600 2>&1 \
  | tee /tmp/acceptance/mesh_grant_c_to_a.txt

# Verify the peer registry has no private keys
python3 -c "
import json
try:
    with open('$MESH_REGISTRY') as f:
        reg = json.load(f)
    for peer_id, info in reg.items() if isinstance(reg, dict) else enumerate(reg):
        info_str = json.dumps(info)
        if 'private' in info_str.lower() or 'secret' in info_str.lower():
            print(f'FAIL: private/secret key found for {peer_id}')
        else:
            print(f'OK: {peer_id} — no private keys')
except Exception as e:
    print(f'Registry check error: {e}')" \
  | tee /tmp/acceptance/mesh_registry_no_secrets.txt
```

---

## Phase 10C: DIDComm Sovereign Protocol (All 7 Message Types)

### 10C.1-10C.10 — All DIDComm Tests

```bash
# Run ALL DIDComm tests individually with output
for test in \
  sim_didcomm_encrypt_decrypt_all_pairs \
  sim_didcomm_cross_agent_isolation \
  sim_didcomm_tamper_detection \
  sim_mcp_tool_call_roundtrip \
  sim_proof_envelope_exchange \
  sim_capability_grant_and_accept \
  sim_heartbeat_three_node \
  sim_session_lifecycle_three_nodes \
  sim_session_capability_tracking \
  sim_full_scenario_grant_query_exchange \
  sim_full_scenario_multi_tool_burst; do
  echo "=== $test ===" | tee -a /tmp/acceptance/didcomm_all_tests.txt
  cargo test --release "$test" -- --nocapture 2>&1 | tee -a /tmp/acceptance/didcomm_all_tests.txt
done

# Summarize
grep -E '^test |^=== |test result:' /tmp/acceptance/didcomm_all_tests.txt \
  | tee /tmp/acceptance/didcomm_summary.txt
```

---

## Phase 11: Dashboard API Coverage

### Comprehensive API Endpoint Sweep

```bash
KEY_A="test-api-key-instance-a"
BASE="http://127.0.0.1:3100"

# All known API routes — test each one
declare -A ENDPOINTS=(
  ["/api/status"]="GET"
  ["/api/setup/state"]="GET"
  ["/api/sessions"]="GET"
  ["/api/costs"]="GET"
  ["/api/config"]="GET"
  ["/api/trust"]="GET"
  ["/api/attestations"]="GET"
  ["/api/genesis/status"]="GET"
  ["/api/vault/keys"]="GET"
  ["/api/nucleusdb/browse"]="GET"
  ["/api/nucleusdb/commits"]="GET"
  ["/api/nucleusdb/vectors"]="GET"
  ["/api/nucleusdb/proofs"]="GET"
  ["/api/nucleusdb/sharing"]="GET"
  ["/api/cockpit/sessions"]="GET"
  ["/api/deploy/catalog"]="GET"
  ["/api/proxy/v1/models"]="GET"
  ["/api/x402/status"]="GET"
)

echo "API Endpoint Sweep:" > /tmp/acceptance/api_sweep.txt
for endpoint in "${!ENDPOINTS[@]}"; do
  METHOD="${ENDPOINTS[$endpoint]}"
  RESPONSE=$(curl -sf -w "\n%{http_code}" -H "Authorization: Bearer $KEY_A" "${BASE}${endpoint}" 2>&1)
  HTTP_CODE=$(echo "$RESPONSE" | tail -1)
  CONTENT_TYPE=$(curl -sI -H "Authorization: Bearer $KEY_A" "${BASE}${endpoint}" 2>&1 | grep -i content-type | head -1)
  IS_JSON=$(echo "$RESPONSE" | head -1 | python3 -c "import json,sys; json.load(sys.stdin); print('YES')" 2>/dev/null || echo "NO")
  echo "$endpoint [$METHOD] → HTTP $HTTP_CODE | JSON=$IS_JSON | $CONTENT_TYPE" | tee -a /tmp/acceptance/api_sweep.txt
done

# Cockpit allowlist enforcement
curl -s -X POST -H "Authorization: Bearer $KEY_A" \
  -H "Content-Type: application/json" \
  -d '{"command":"rm -rf /"}' \
  ${BASE}/api/cockpit/sessions > /tmp/acceptance/cockpit_blocked.txt 2>&1
echo "Cockpit blocked: $(cat /tmp/acceptance/cockpit_blocked.txt)"

curl -s -X POST -H "Authorization: Bearer $KEY_A" \
  -H "Content-Type: application/json" \
  -d '{"command":"bash -c id"}' \
  ${BASE}/api/cockpit/sessions > /tmp/acceptance/cockpit_shellc_blocked.txt 2>&1

# Deploy catalog
curl -s ${BASE}/api/deploy/catalog | python3 -m json.tool > /tmp/acceptance/deploy_catalog.json 2>&1
```

---

## Phase 12: Proxy (OpenAI-Compatible)

```bash
KEY_A="test-api-key-instance-a"
BASE="http://127.0.0.1:3100"

# Model list
curl -s -H "Authorization: Bearer $KEY_A" ${BASE}/api/proxy/v1/models \
  | python3 -m json.tool > /tmp/acceptance/proxy_models.json 2>&1

# Proxy without key — verify clear error, NO API key in error
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo vault delete openrouter 2>/dev/null
curl -s -X POST -H "Authorization: Bearer $KEY_A" \
  -H "Content-Type: application/json" \
  -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"test"}],"stream":false}' \
  ${BASE}/api/proxy/v1/chat/completions > /tmp/acceptance/proxy_no_key.json 2>&1

# Verify error does NOT leak API keys
python3 -c "
import json
with open('/tmp/acceptance/proxy_no_key.json') as f:
    resp = f.read()
if 'sk-or-' in resp or 'sk-' in resp.lower():
    print('FAIL: API key leaked in error response')
else:
    print('PASS: No API key in error response')" \
  | tee /tmp/acceptance/proxy_key_leak_check.txt

# Note: chat completion with real key requires $OPENROUTER_KEY to be set.
# If OPENROUTER_KEY is available, test non-streaming and streaming:
if [ -n "$OPENROUTER_KEY" ]; then
  AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo vault set openrouter "$OPENROUTER_KEY"

  # Non-streaming
  curl -s -X POST -H "Authorization: Bearer $KEY_A" \
    -H "Content-Type: application/json" \
    -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Say hello in 5 words"}],"stream":false}' \
    ${BASE}/api/proxy/v1/chat/completions > /tmp/acceptance/proxy_nonstream.json 2>&1

  # Streaming
  timeout 10 curl -s -N -X POST -H "Authorization: Bearer $KEY_A" \
    -H "Content-Type: application/json" \
    -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Count 1 to 5"}],"stream":true}' \
    ${BASE}/api/proxy/v1/chat/completions > /tmp/acceptance/proxy_stream.txt 2>&1

  # Verify trace recorded
  AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo traces --json > /tmp/acceptance/traces_after_proxy.json
  echo "Traces after proxy call: $(python3 -c "import json; print(len(json.load(open('/tmp/acceptance/traces_after_proxy.json'))))")"
fi
```

---

## Phase 13: On-Chain Integration (Simulation)

```bash
AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_ONCHAIN_SIMULATION=1 \
  target/release/agenthalo onchain config \
  --rpc-url "https://sepolia.base.org" \
  --chain-id 84532 \
  --contract "0x0000000000000000000000000000000000000000" \
  --signer-mode private_key_env \
  --private-key-env AGENTHALO_ONCHAIN_PRIVATE_KEY 2>&1 \
  | tee /tmp/acceptance/onchain_config.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" AGENTHALO_ONCHAIN_SIMULATION=1 \
  target/release/agenthalo attest --session "$SESSION_A" --onchain 2>&1 \
  | tee /tmp/acceptance/onchain_attest.txt

# Verify domain separator is NOT "stub"
python3 -c "
content = open('/tmp/acceptance/onchain_attest.txt').read()
if 'stub' in content.lower():
    print('FAIL: domain separator contains stub')
elif 'simulation' in content.lower():
    print('PASS: simulation mode correctly labeled')
else:
    print('WARN: check manually')" \
  | tee /tmp/acceptance/onchain_domain_check.txt

AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo vote \
  --proposal "QA-TEST-001" --choice yes --reason "testing governance flow" 2>&1 \
  | tee /tmp/acceptance/vote.txt
```

---

## Phase 14: MCP Server (Full Tool Coverage)

```bash
# Start MCP server on port 3400
AGENTHALO_HOME="$AGENTHALO_HOME_A" \
  AGENTHALO_ALLOW_DEV_SECRET=1 \
  AGENTHALO_MCP_SECRET="test-mcp-secret-qa" \
  NUCLEUSDB_MESH_AGENT_ID="instance-a" \
  NUCLEUSDB_MESH_REGISTRY="$MESH_REGISTRY" \
  target/release/agenthalo-mcp-server --port 3400 &
MCP_PID=$!
sleep 2

MCP_SECRET="test-mcp-secret-qa"
MCP_BASE="http://127.0.0.1:3400"

# List ALL tools
curl -s -H "Authorization: Bearer $MCP_SECRET" $MCP_BASE/tools \
  | python3 -m json.tool > /tmp/acceptance/mcp_tools_list.json 2>&1
TOOL_COUNT=$(python3 -c "import json; print(len(json.load(open('/tmp/acceptance/mcp_tools_list.json'))))" 2>/dev/null)
echo "MCP tool count: $TOOL_COUNT" | tee /tmp/acceptance/mcp_tool_count.txt

# Test core tools
for tool in halo_status crypto_status identity_status genesis_status proof_gate_status trust_query; do
  echo "=== MCP: $tool ==="
  curl -s -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $MCP_SECRET" \
    -d "{\"name\":\"$tool\",\"arguments\":{}}" \
    $MCP_BASE/call 2>&1 | python3 -m json.tool
done | tee /tmp/acceptance/mcp_core_tools.txt

# Test mesh tools (may or may not be registered)
for tool in mesh_peers mesh_ping mesh_call mesh_exchange_envelope mesh_grant; do
  echo "=== MCP mesh: $tool ==="
  curl -s -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $MCP_SECRET" \
    -d "{\"name\":\"$tool\",\"arguments\":{}}" \
    $MCP_BASE/call 2>&1
done | tee /tmp/acceptance/mcp_mesh_tools.txt

# Test ZK tools
for tool in proof_gate_status proof_gate_verify zk_prove_credential zk_verify_credential zk_compute_prove zk_compute_verify; do
  echo "=== MCP zk: $tool ==="
  curl -s -X POST -H "Content-Type: application/json" -H "Authorization: Bearer $MCP_SECRET" \
    -d "{\"name\":\"$tool\",\"arguments\":{}}" \
    $MCP_BASE/call 2>&1
done | tee /tmp/acceptance/mcp_zk_tools.txt

kill $MCP_PID 2>/dev/null

# NucleusDB MCP Server
target/release/nucleusdb-mcp --help > /tmp/acceptance/nucleusdb_mcp_help.txt 2>&1 || true
```

---

## Phase 15: Access Control & Capabilities

```bash
# Grant
GRANT_OUT=$(AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access grant \
  "did:key:agent-b" "traces" --modes "read" --ttl 3600 2>&1)
echo "$GRANT_OUT" | tee /tmp/acceptance/access_grant.txt
TOKEN_ID=$(echo "$GRANT_OUT" | python3 -c "import json,sys; print(json.load(sys.stdin).get('token_id',''))" 2>/dev/null)

# List grants
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access list --grantee "did:key:agent-b" 2>&1 \
  | tee /tmp/acceptance/access_list.txt

# Policy evaluate — read should pass
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access policy evaluate \
  "did:key:agent-b" "traces" "read" 2>&1 \
  | tee /tmp/acceptance/access_eval_read.txt

# Policy evaluate — write should fail
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access policy evaluate \
  "did:key:agent-b" "traces" "write" 2>&1 \
  | tee /tmp/acceptance/access_eval_write.txt

# Revoke
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access revoke "$TOKEN_ID" 2>&1 \
  | tee /tmp/acceptance/access_revoke.txt

# Verify revoked
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo access list --grantee "did:key:agent-b" 2>&1 \
  | tee /tmp/acceptance/access_list_after_revoke.txt
```

---

## Phase 15B: POD (Proof of Data) Features

```bash
# Identity POD share
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo identity pod-share \
  --recipient "did:key:qa-recipient-001" \
  --patterns "profile.name,device.fingerprint" 2>&1 \
  | tee /tmp/acceptance/pod_share.txt

# Verify sharing record in identity status
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo identity status --json \
  > /tmp/acceptance/identity_after_pod.json 2>&1

# Dashboard POD endpoints
curl -s -H "Authorization: Bearer test-api-key-instance-a" \
  http://127.0.0.1:3100/api/nucleusdb/sharing > /tmp/acceptance/pod_sharing_api.json 2>&1
curl -s -H "Authorization: Bearer test-api-key-instance-a" \
  http://127.0.0.1:3100/api/nucleusdb/proofs > /tmp/acceptance/pod_proofs_api.json 2>&1
```

---

## Phase 16: x402 Payment Protocol

```bash
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo x402 status 2>&1 \
  | tee /tmp/acceptance/x402_status.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo x402 config 2>&1 \
  | tee /tmp/acceptance/x402_config.txt
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo x402 balance 2>&1 \
  | tee /tmp/acceptance/x402_balance.txt
```

---

## Phase 17: NucleusDB CLI Standalone

```bash
# SQL operations
target/release/nucleusdb --db /tmp/acceptance/qa_ndb.db <<'SQL' > /tmp/acceptance/ndb_sql.txt 2>&1
CREATE TABLE users (name TEXT, age INTEGER);
INSERT INTO users (name, age) VALUES ('Alice', 30);
INSERT INTO users (name, age) VALUES ('Bob', 25);
COMMIT;
SELECT * FROM users;
SELECT * FROM users WHERE age > 27;
SQL
echo "SQL exit: $?"

# Vector search
target/release/nucleusdb --db /tmp/acceptance/qa_ndb_vec.db --backend ipa <<'SQL' > /tmp/acceptance/ndb_vector.txt 2>&1
SET doc1 VECTOR [1.0, 0.0, 0.0];
SET doc2 VECTOR [0.0, 1.0, 0.0];
SET doc3 VECTOR [0.7, 0.7, 0.0];
COMMIT;
KNN doc1 K=2;
SQL
echo "Vector exit: $?"

# Blob store
target/release/nucleusdb --db /tmp/acceptance/qa_ndb_blob.db <<'SQL' > /tmp/acceptance/ndb_blob.txt 2>&1
BLOB PUT 'hello world';
SQL
echo "Blob exit: $?"

# NucleusDB status + export
target/release/nucleusdb --db /tmp/acceptance/qa_ndb.db status > /tmp/acceptance/ndb_status.txt 2>&1
target/release/nucleusdb --db /tmp/acceptance/qa_ndb.db export > /tmp/acceptance/ndb_export.json 2>&1

# CAB license verify (if applicable)
target/release/nucleusdb license --help > /tmp/acceptance/ndb_license_help.txt 2>&1

# TUI launches (quick check)
timeout 3 target/release/nucleusdb-tui --help > /tmp/acceptance/ndb_tui_help.txt 2>&1 || true
```

---

## Phase 18: Security Audit Checklist (30 Items)

Run each check individually:

```bash
echo "=== Security Audit ===" > /tmp/acceptance/security_audit.txt

# S1: API keys never in error messages
echo "S1: API key redaction" >> /tmp/acceptance/security_audit.txt
grep -i 'sk-or-\|sk-' /tmp/acceptance/proxy_no_key.json && echo "FAIL" >> /tmp/acceptance/security_audit.txt || echo "PASS" >> /tmp/acceptance/security_audit.txt

# S2: Vault file encrypted
echo "S2: Vault encryption" >> /tmp/acceptance/security_audit.txt
file "$AGENTHALO_HOME_A/vault.enc" 2>&1 >> /tmp/acceptance/security_audit.txt || echo "N/A" >> /tmp/acceptance/security_audit.txt

# S3: Auth required for sensitive endpoints
echo "S3: Auth required" >> /tmp/acceptance/security_audit.txt
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3100/api/vault/keys)
echo "  /api/vault/keys without auth: $HTTP_CODE (expect 401/403)" >> /tmp/acceptance/security_audit.txt

# S4: Cockpit command allowlist
echo "S4: Cockpit allowlist" >> /tmp/acceptance/security_audit.txt
cat /tmp/acceptance/cockpit_blocked.txt >> /tmp/acceptance/security_audit.txt

# S5: Shell -c blocked
echo "S5: Shell -c blocked" >> /tmp/acceptance/security_audit.txt
cat /tmp/acceptance/cockpit_shellc_blocked.txt >> /tmp/acceptance/security_audit.txt

# S6: Domain separators — no sim_/stub_ abbreviations
echo "S6: Domain separators" >> /tmp/acceptance/security_audit.txt
grep -rn 'sim_\|stub_\|\.sim\.\|\.stub\.' src/ 2>/dev/null | grep -v 'simulation\|sim_mesh\|sim_didcomm\|sim_mcp\|sim_proof\|sim_capability\|sim_heartbeat\|sim_session\|sim_full' >> /tmp/acceptance/security_audit.txt || echo "PASS" >> /tmp/acceptance/security_audit.txt

# S7-S30: covered by evidence from previous phases
# Enumerate them explicitly:
for check in \
  "S7: Monotone seal chain tamper-evident → seal_chain_result.txt" \
  "S8: PQ signatures verify → sign_pq_a.txt" \
  "S9: Genesis seeds unique → genesis_uniqueness.txt" \
  "S10: Data isolation → trace_isolation.txt" \
  "S11: Simulation labeled → onchain_domain_check.txt" \
  "S12: Identity ledger chain → identity_chain_check.txt" \
  "S13: No sorry/admit → lean_sorry_scan.txt" \
  "S14: Concurrent trace DB → immutability_count.txt" \
  "S15: ZK credential round-trip → zk_credential_verify.txt" \
  "S16: ZK anonymous membership → zk_anon_verify.txt" \
  "S17: ZK compute tamper detection → zk_compute_tampered_verify.txt" \
  "S18: Lean proof gate blocks → proof_gate_without_cert.txt" \
  "S19: Lean specs build → lean_build.txt" \
  "S20: POD share scoped → pod_share.txt" \
  "S21: Capability token scoping → access_eval_write.txt" \
  "S22: Cross-instance ZK isolation → zk_cross_instance_verify.txt" \
  "S23: DIDComm cross-agent isolation → didcomm_all_tests.txt" \
  "S24: DIDComm tamper detection → didcomm_all_tests.txt" \
  "S25: DIDComm dual signature → didcomm_all_tests.txt" \
  "S26: Mesh capability expiry → mesh_sim_tests.txt" \
  "S27: Mesh wildcard vs exact → mesh_sim_tests.txt" \
  "S28: DIDComm message ID uniqueness → didcomm_all_tests.txt" \
  "S29: Mesh registry no private keys → mesh_registry_no_secrets.txt" \
  "S30: Three-instance data isolation → genesis_uniqueness.txt + trace_isolation.txt"; do
  echo "$check" >> /tmp/acceptance/security_audit.txt
done
```

---

## Phase 19: Cross-Instance Interaction

```bash
# Export attestation from A, verify self-contained
AGENTHALO_HOME="$AGENTHALO_HOME_A" target/release/agenthalo attest --session "$SESSION_A" \
  > /tmp/acceptance/attestation_cross_a.json 2>&1

# Doctor on all three
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  AGENTHALO_HOME="$HOME_VAR" target/release/agenthalo doctor 2>&1 \
    | tee /tmp/acceptance/doctor_${inst}.txt
done

# Full 3-node simulation suite (re-run to confirm all 15 pass at end)
cargo test --release mesh_simulation -- --nocapture 2>&1 | tee /tmp/acceptance/mesh_final_suite.txt
grep 'test result:' /tmp/acceptance/mesh_final_suite.txt

# Lean formal spec integrity (verify Comms theorems)
echo "=== Lean Comms Theorems ===" | tee /tmp/acceptance/lean_comms_theorems.txt
grep -rn '^theorem' lean/NucleusDB/Comms/ | tee -a /tmp/acceptance/lean_comms_theorems.txt
echo "=== Lean ZK Theorems ===" | tee -a /tmp/acceptance/lean_comms_theorems.txt
grep -rn '^theorem' lean/NucleusDB/Comms/ZK/ | tee -a /tmp/acceptance/lean_comms_theorems.txt
echo "=== Lean Security Theorems ===" | tee -a /tmp/acceptance/lean_comms_theorems.txt
grep -rn '^theorem' lean/NucleusDB/Security/ | tee -a /tmp/acceptance/lean_comms_theorems.txt
```

---

## Phase 20: PUF (Physically Unclonable Function) Fingerprinting

```bash
# Run PUF tests
cargo test --release puf -- --nocapture 2>&1 | tee /tmp/acceptance/puf_tests.txt

# PUF auto-detect
python3 -c "
# PUF tier detection happens at runtime
# Verify via challenge-response stability
print('PUF test suite covers: core, tpm, dgx, server, consumer, wasm')" \
  | tee /tmp/acceptance/puf_summary.txt
```

---

## Phase 21: Additional Test Suites (Full Coverage)

Run every integration test file to ensure full coverage:

```bash
# All test files
for test_file in \
  abraxas_merge_agent_tests \
  abraxas_vcs_tests \
  cli_smoke_tests \
  container_tests \
  dashboard_tests \
  end_to_end \
  halo_integration \
  keymap_tests \
  mesh_simulation_test \
  pcn_tests \
  persistence_compat_tests \
  puf_tests \
  sql_tests; do
  echo "=== $test_file ===" | tee -a /tmp/acceptance/all_integration_tests.txt
  cargo test --release --test "$test_file" -- --nocapture 2>&1 \
    | grep -E 'test |test result:' | tee -a /tmp/acceptance/all_integration_tests.txt
done

# Payment channel tests
cargo test --release pcn -- --nocapture 2>&1 | tee /tmp/acceptance/pcn_tests.txt

# VCS (version control) tests
cargo test --release vcs -- --nocapture 2>&1 | tee /tmp/acceptance/vcs_tests.txt

# Composite CAB tests
cargo test --release composite_cab -- --nocapture 2>&1 | tee /tmp/acceptance/cab_tests.txt

# Transparency CT6962 tests
cargo test --release ct6962 -- --nocapture 2>&1 | tee /tmp/acceptance/ct6962_tests_detail.txt
```

---

## Phase 22: Post-Test Log & Database Inspection

**After ALL phases, inspect every artifact for anomalies:**

```bash
echo "=== POST-TEST INSPECTION ===" > /tmp/acceptance/post_inspection.txt

# 1. Trace database sizes
for inst in A B C; do
  eval HOME_VAR=\$AGENTHALO_HOME_$inst
  echo "--- Instance $inst ---" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR"/ >> /tmp/acceptance/post_inspection.txt 2>&1
  echo "  Trace DB:" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR/traces.redb" >> /tmp/acceptance/post_inspection.txt 2>&1
  echo "  Genesis seed:" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR/genesis_seed.enc" >> /tmp/acceptance/post_inspection.txt 2>&1
  echo "  PQ wallet:" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR/pq_wallet.json" >> /tmp/acceptance/post_inspection.txt 2>&1
  echo "  Vault:" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR/vault.enc" >> /tmp/acceptance/post_inspection.txt 2>&1
  echo "  Identity:" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR/identity"* >> /tmp/acceptance/post_inspection.txt 2>&1
  echo "  Proof certificates:" >> /tmp/acceptance/post_inspection.txt
  ls -la "$HOME_VAR/proof_certificates/" >> /tmp/acceptance/post_inspection.txt 2>&1
done

# 2. Mesh registry contents
echo "--- Mesh Registry ---" >> /tmp/acceptance/post_inspection.txt
cat "$MESH_REGISTRY" 2>/dev/null | python3 -m json.tool >> /tmp/acceptance/post_inspection.txt 2>&1

# 3. NucleusDB databases
echo "--- NucleusDB test databases ---" >> /tmp/acceptance/post_inspection.txt
for db in /tmp/acceptance/qa_*.db; do
  echo "  $db: $(ls -la "$db" 2>&1)" >> /tmp/acceptance/post_inspection.txt
done

# 4. ZK proof artifacts
echo "--- ZK artifacts ---" >> /tmp/acceptance/post_inspection.txt
for zk in /tmp/acceptance/zk_*.json; do
  SIZE=$(wc -c < "$zk" 2>/dev/null || echo 0)
  IS_JSON=$(python3 -c "import json; json.load(open('$zk')); print('valid')" 2>/dev/null || echo "invalid")
  echo "  $zk: ${SIZE}B, $IS_JSON" >> /tmp/acceptance/post_inspection.txt
done

# 5. Verify no plaintext secrets in any artifact
echo "--- Secret leak scan ---" >> /tmp/acceptance/post_inspection.txt
grep -rl 'sk-or-\|private_key\|secret_key' /tmp/acceptance/ 2>/dev/null \
  | grep -v '.txt$\|_check\|_leak\|_audit' >> /tmp/acceptance/post_inspection.txt \
  || echo "PASS: no secret leaks found" >> /tmp/acceptance/post_inspection.txt

# 6. Count total evidence files
echo "--- Evidence inventory ---" >> /tmp/acceptance/post_inspection.txt
echo "Total evidence files: $(ls /tmp/acceptance/ | wc -l)" >> /tmp/acceptance/post_inspection.txt

cat /tmp/acceptance/post_inspection.txt
```

---

## Phase 23: Cleanup

```bash
# Kill all instances
kill $PID_A $PID_B $PID_C 2>/dev/null
pkill -f 'agenthalo' || true
pkill -f 'nucleusdb' || true

# Verify no orphan processes
sleep 2
pgrep -fa 'agenthalo|nucleusdb' && echo "WARN: orphan processes found" || echo "All processes cleaned up"
```

---

## Deliverable: Structured Report

Save to: `WIP/agenthalo_definitive_acceptance_test_report_YYYYMMDD.md`

The report MUST include:

1. **Executive Summary** (1-3 sentences: overall production readiness)
2. **Phase Scorecard** (23 phases + sub-phases, PASS/FAIL/PARTIAL, notes)
3. **Errors Found** (numbered E1-EN, with severity/phase/reproduction/expected/actual/fix)
4. **UX Friction Points** (numbered U1-UN)
5. **Missing Capabilities / Suggestions** (numbered F1-FN)
6. **Security Audit Results** (S1-S30, PASS/FAIL, evidence file)
7. **MCP Tool Coverage** (every tool, PASS/FAIL/MISSING)
8. **Cryptographic Hardness Verification** (22 items from the matrix)
9. **Communication Channel Status** (7 channels)
10. **Lean Formal Proof Surface** (theorem count, domain breakdown, sorry/admit scan)
11. **ZK Guest Computation Coverage** (4 builtin guests, tamper detection)
12. **Nym Privacy Transport** (fail-open/fail-closed, classification)
13. **Instance Isolation Verification** (genesis seeds, vaults, traces, DIDs)
14. **Post-Test Log/Database Inspection** (all artifacts verified)
15. **Improvement Backlog** (prioritized: CRITICAL/HIGH/MEDIUM/LOW)

---

## Comparison with v1 Instructions

This v2 plan adds or strengthens:

| Area | v1 | v2 |
|------|----|----|
| Workflow ordering | Implicit (caused seed-wrap bug) | Explicit: genesis creates PQ wallet, never keygen after |
| Mesh env vars | Assumed Docker | Explicit env vars on every command |
| ZK guests | Only range_proof tested | All 4 builtin guests tested |
| ZK tamper detection | 1 test | 1 + image ID derivation tests |
| Lean theorem audit | Count only | Full domain breakdown + named theorem list |
| PUF tests | Not covered | Phase 20 |
| CT6962 transparency | Not covered | Phase 6.4 |
| PCN/VCS/CAB tests | Not covered | Phase 21 |
| Nym fail-closed | Mentioned | Explicit test |
| Post-test inspection | Not covered | Phase 22 (all logs, DBs, artifacts, secret scan) |
| API sweep | 7 endpoints | All 18+ known endpoints with content-type check |
| Cross-instance ZK | Mentioned | Explicit verify-on-wrong-instance test |
| Evidence management | /tmp files scattered | /tmp/acceptance/ with descriptive names |
| DIDComm tests | Individual | All 11 tests run individually with output |
| Security audit | 18 checks | 30 checks (added DIDComm, mesh, PUF, transparency) |
