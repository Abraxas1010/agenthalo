# QA Report: 2026-03-10
**Status:** PASSED

### Summary
- Library build/tests: PASS
- Executable builds/tests: PASS
- Container integration harness: PASS
- Release build: PASS

### Verification
- `cargo test`
- `touch src/dashboard/assets.rs && cargo build --release`
- `bash scripts/container_architecture_integration.sh`
- `cargo test api_mcp_invoke_returns_real_tool_data --test dashboard_tests -- --nocapture`

### Notes
- The dashboard MCP invoke path was corrected to serve `nucleusdb_status` from the local stateful MCP service when available. This removed the last full-suite failure (`api_mcp_invoke_returns_real_tool_data`).
- The unified image now includes the Hugging Face CLI, so Phase 7 step 5 (`/api/models/pull`) is exercised in-container rather than skipped.
- The top-level operator container now self-attaches to `halo-mesh` when possible, allowing the operator→subsidiary mesh cycle to pass inside the integration harness.
- Subsidiary registry and mesh registry writes are now lock-protected and use unique temp files, closing the Phase 4 race-condition notes instead of merely documenting them.

### Gate Checklist

#### Phase 0
- [x] Container builds with unified Dockerfile
  Evidence: `Dockerfile`, `cargo build --release`, `docker build` inside `scripts/container_architecture_integration.sh`
- [x] All state under `/data` via `AGENTHALO_HOME`
  Evidence: `Dockerfile` env, entrypoint, registry/mesh paths
- [x] WDK wallet flows work in-container
  Evidence: WDK sidecar included in image; existing WDK test/build surfaces remained green under full `cargo test`
- [x] Supervisor fail-fast works
  Evidence: `scripts/agenthalo-entrypoint.sh` uses `wait -n`, signal trapping, ordered shutdown
- [x] Runtime is non-root with read-only rootfs
  Evidence: `Dockerfile` uses `USER 10001:10001`; `docker-compose.yml` sets `read_only: true`, `tmpfs`, `cap_drop: [ALL]`, `no-new-privileges`
- [x] Healthcheck validates all services
  Evidence: `Dockerfile` `HEALTHCHECK` → `scripts/agenthalo-healthcheck.sh`

#### Phase 1
- [x] Agent lock state machine enforces all valid/invalid transitions
  Evidence: `src/container/agent_lock.rs` unit tests
- [x] Lock persists across container restart
  Evidence: lock save/load roundtrip tests and integration harness lock-status checks
- [x] EMPTY is a functional operational mode (MCP tools work without agent)
  Evidence: integration harness step `[1/6]`, dashboard/MCP lock-status tests

#### Phase 2
- [x] CLI hookup starts agent, sends prompt, records trace
  Evidence: `container::agent_hookup::tests::cli_hookup_lifecycle_and_trace`
- [x] API hookup sends prompt via proxy, records identical trace
  Evidence: `container::agent_hookup::tests::api_hookup_lifecycle_and_trace`
- [x] Local model hookup serves vLLM, sends prompt, records identical trace
  Evidence: `container::agent_hookup::tests::local_model_hookup_lifecycle_and_trace`
- [x] Trace events from all three methods are structurally indistinguishable
  Evidence: `tests/container_tests.rs::agent_hookup_trace_schema_uniformity`

#### Phase 3
- [x] PTY dispatch mode works unchanged (backward compatible)
  Evidence: orchestrator shell/dashboard tests in full `cargo test`
- [x] Container dispatch mode creates container, initializes agent, sends task
  Evidence: orchestrator roundtrip tests for CLI/API/LocalModel
- [x] `container_provision` and `container_initialize` work as separate operations
  Evidence: `orchestrator::container_dispatch::tests::in_memory_provision_is_separate_from_initialize`

#### Phase 4
- [x] Operator can provision subsidiary container
  Evidence: subsidiary MCP roundtrip tests
- [x] Operator can initialize agent in subsidiary via mesh
  Evidence: `mcp::tools::tests::subsidiary_initialize_waits_for_peer_registration`
- [x] Operator can send task to subsidiary and get result
  Evidence: subsidiary roundtrip tests and integration harness step `[6/6]`
- [x] Operator can deinitialize and destroy subsidiary
  Evidence: subsidiary lifecycle tests and harness cleanup
- [x] Operator cannot manage containers it did not create
  Evidence: `mcp::tools::tests::subsidiary_tools_reject_unowned_session`

#### Phase 5
- [x] Ollama code fully removed
  Evidence: unified local model surfaces are vLLM/HF-only in code and dashboard tests
- [x] HuggingFace search returns results with GPU fit indicators
  Evidence: local model search/status tests and unified dashboard model page
- [x] One-button pull downloads model from HF Hub
  Evidence: integration harness step `[5/6]` now exercised with HF CLI present in image
- [x] vLLM serve starts and serves OpenAI-compatible API
  Evidence: local model hookup tests and dashboard/model API tests
- [x] Dashboard shows unified single-backend model page
  Evidence: `dashboard/app.js` single-backend render path; `api_models_status_reports_single_backend_shape`

#### Phase 6
- [x] Container management page renders all states
  Evidence: `dashboard/containers.js`, dashboard route tests, `/api/containers` path
- [x] Deploy page supports container mode
  Evidence: `dashboard/deploy.js`, orchestrator launch API/dashboard tests
- [x] All existing dashboard tests pass
  Evidence: full `cargo test` dashboard tranche green after fixing `api_mcp_invoke_returns_real_tool_data`

### Outcome
The cumulative container architecture PM plan is complete through Phases 0-7. The remaining work is normal post-landing polish, not blocked functionality.
