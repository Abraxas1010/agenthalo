# Orchestrator Debugging Playbook

This runbook is for diagnosing orchestrator failures in MCP HTTP deployments.

## Fast Path

1. Confirm server health:
   - `curl -sf http://127.0.0.1:9876/health`
2. Validate MCP protocol handshake:
   - `python3 scripts/mcp_streamable_http.py --endpoint http://127.0.0.1:9876/mcp init --session-file /tmp/mcp.session`
3. Run full smoke:
   - `scripts/orchestrator_mcp_smoke.sh`

If smoke passes, core launch/task/trace/stop path is healthy.

## Common Failure Modes

## 1) `406 Not Acceptable` on `/mcp`

Cause:
- Missing or incorrect `Accept` header.

Fix:
- Require `Accept: application/json, text/event-stream`.
- Prefer `scripts/mcp_streamable_http.py` over ad-hoc curl.

## 2) Tool calls return empty/non-actionable SSE payloads

Cause:
- Missing `mcp-session-id` continuity after `initialize`.

Fix:
- Persist `mcp-session-id` and replay it for all subsequent calls.

## 3) `trace=true` + `wait=true` shell tasks time out intermittently

Cause:
- Fast-exit race where output or terminal status arrives before subscriber attach.

Current mitigation in code:
- PTY sessions buffer captured output (`snapshot_output()`).
- `collect_task_output()` seeds from snapshot and polls status in short intervals.
- Regression tests:
  - `collect_task_output_handles_fast_exit_before_subscribe`
  - `orchestrator_shell_trace_wait_roundtrip_multiple_tasks`

## 4) `trace_session_id` present but SQL reads show no trace rows

Cause:
- Runtime service SQL may read from an in-memory DB instance not reloaded from
  persisted trace snapshot yet.

Fix:
- Treat persisted trace store as source of truth for audit.
- Reload DB state or read trace store directly for verification workflows.

## 5) Proxy mode websocket output unavailable (`501`)

Cause:
- In proxy mode, dashboard cannot subscribe directly to remote PTY stream.

Fix:
- Use task-status polling and result retrieval through MCP tools.
- This is an expected degradation in proxy-first deployments.

## Diagnostic Commands

Launch a traced shell agent:

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-call --session-file /tmp/mcp.session \
  --tool orchestrator_launch \
  --arguments '{"agent":"shell","agent_name":"dbg","timeout_secs":30,"trace":true}'
```

Run a wait-mode task:

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-call --session-file /tmp/mcp.session \
  --tool orchestrator_send_task \
  --arguments '{"agent_id":"orch-...","task":"echo debug","wait":true,"timeout_secs":20}'
```

List active agents/tasks:

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-call --session-file /tmp/mcp.session --tool orchestrator_list

python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-call --session-file /tmp/mcp.session --tool orchestrator_tasks
```

## Verification Before Shipping

- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `scripts/orchestrator_mcp_smoke.sh`
