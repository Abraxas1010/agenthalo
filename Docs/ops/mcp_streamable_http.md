# MCP Streamable HTTP Notes

This note captures the protocol details that frequently break manual testing.

## Required Request Headers

For `POST /mcp` calls, clients must send:

- `Content-Type: application/json`
- `Accept: application/json, text/event-stream`

If `Accept` does not include both media types, the server may return:

- `406 Not Acceptable: Client must accept both application/json and text/event-stream`

## Session Lifecycle

1. Call `initialize`
2. Read `mcp-session-id` response header
3. Reuse that `mcp-session-id` on subsequent `tools/list` and `tools/call` requests

Without a valid session id, tool calls can fail or return empty SSE keepalive frames.

## Built-In Helper Script

Use `scripts/mcp_streamable_http.py` to avoid brittle manual SSE parsing.

### Initialize

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  init --session-file /tmp/mcp.session
```

### List Tools

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-list --session-file /tmp/mcp.session
```

### Call Tool

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-call --session-file /tmp/mcp.session \
  --tool status
```

With arguments:

```bash
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:9876/mcp \
  tools-call --session-file /tmp/mcp.session \
  --tool orchestrator_send_task \
  --arguments '{"agent_id":"orch-...","task":"echo hi","wait":true}'
```

## End-to-End Orchestrator Smoke

Run:

```bash
scripts/orchestrator_mcp_smoke.sh
```

This script starts `nucleusdb-mcp`, performs `initialize`, launches a traced shell
agent, executes multiple tasks through `tools/call`, validates completion and
`trace_session_id`, then stops the agent.
