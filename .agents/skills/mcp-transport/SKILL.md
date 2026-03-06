# Skill: mcp-transport

> **Trigger:** MCP protocol, MCP session, SSE response, MCP initialize, MCP handshake, streamable HTTP, MCP error, session expired
> **Category:** infrastructure
> **Audience:** External (controlling agent calling MCP over HTTP)

## Purpose

Definitive reference for the MCP (Model Context Protocol) transport layer used by `nucleusdb-mcp` in HTTP mode. This skill prevents the most common integration errors: session lifecycle failures, SSE response parsing mistakes, and authentication confusion.

---

## Transport: Streamable HTTP (SSE)

NucleusDB MCP uses **Streamable HTTP** transport (not stdio, not WebSocket).

- **Endpoint:** `POST http://host:port/mcp`
- **Content-Type:** `application/json` (request), `text/event-stream` (response)
- **Session tracking:** Via `Mcp-Session-Id` header

---

## Session Lifecycle (MANDATORY)

Every MCP session MUST follow this exact handshake sequence. Skipping any step causes `401 Unauthorized: Session not found`.

### Step 1: Initialize

```bash
curl -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"my-agent","version":"1.0"}}}'
```

Response (SSE format):
```
data: {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"nucleusdb-mcp","version":"0.1.0"}}}
```

**Extract the `Mcp-Session-Id` header from this response.** You need it for all subsequent calls.

### Step 2: Notify Initialized (MANDATORY)

```bash
curl -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: YOUR_SESSION_ID" \
  -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'
```

This is a notification (no `id` field). Response is `202 Accepted` (no body).

**If you skip this step, the session exists but may behave unexpectedly.**

### Step 3: Call Tools

All subsequent `tools/call` requests MUST include the session header:

```bash
curl -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: YOUR_SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"orchestrator_launch","arguments":{...}}}'
```

---

## Parsing SSE Responses

Responses use Server-Sent Events format. **This is the #1 source of parsing errors.**

### Response format

```
data: {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"agent_id\":\"orch-...\"}"}]}}

```

**Rules:**
1. Each data line starts with `data: ` (note the space after colon)
2. Multiple `data:` lines may appear — the **last non-empty** one contains the result
3. Empty lines (`\n\n`) delimit events
4. The actual tool result is inside `result.content[0].text` as a **JSON string**

### Extraction Pattern

```bash
# Extract the tool result from SSE
curl -s -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d @request.json \
  | sed -n 's/^data: //p' \
  | tail -1 \
  | jq -r '.result.content[0].text' \
  | jq .
```

### Python Extraction

```python
import json, requests

response = requests.post(
    "http://localhost:9876/mcp",
    headers={"Content-Type": "application/json", "Mcp-Session-Id": session_id},
    json={"jsonrpc": "2.0", "id": next_id, "method": "tools/call",
          "params": {"name": "orchestrator_launch", "arguments": {...}}},
    stream=True
)

# Parse SSE
last_data = None
for line in response.iter_lines(decode_unicode=True):
    if line.startswith("data: "):
        last_data = line[6:]

result = json.loads(last_data)
tool_output = json.loads(result["result"]["content"][0]["text"])
```

---

## Authentication Modes

| Flag | Behavior |
|------|----------|
| `--no-auth` | No authentication required (dev mode). Warning printed at startup. |
| (default) | Requires `agenthalo auth` credentials or `AGENTHALO_API_KEY` env var |

In auth mode, include the API key:
```bash
-H "Authorization: Bearer $AGENTHALO_API_KEY"
```

---

## Common Errors and Fixes

### `401 Unauthorized: Session not found`

**Cause:** Session expired, or you skipped the initialize handshake, or you're using a stale `Mcp-Session-Id`.

**Fix:** Re-run the full initialize → notifications/initialized → tool call sequence.

### `400 Bad Request: unknown method`

**Cause:** Typo in method name or missing `params` field.

**Fix:** Method must be exactly `"tools/call"`. Params must include `"name"` and `"arguments"`.

### Response is empty or truncated

**Cause:** You're not reading the full SSE stream.

**Fix:** Read until the connection closes. Use `stream=True` in Python requests or `-N` flag with curl.

### Tool result is a string, not an object

**Cause:** MCP wraps tool results in `content[0].text` as a JSON **string**. You need to double-parse.

**Fix:** `json.loads(result["result"]["content"][0]["text"])`

---

## Health Check

```bash
# Verify server is running
curl http://localhost:9876/health
# → {"status":"ok","version":"..."}

# List available tools
curl -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":99,"method":"tools/list","params":{}}'
```

---

## Using Request Files (recommended for complex payloads)

For requests with nested JSON or special characters, write the payload to a file:

```bash
cat > /tmp/mcp_request.json << 'EOF'
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "tools/call",
  "params": {
    "name": "orchestrator_send_task",
    "arguments": {
      "agent_id": "orch-abc",
      "task": "Read the file and review it",
      "wait": true,
      "timeout_secs": 120
    }
  }
}
EOF

curl -X POST http://localhost:9876/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d @/tmp/mcp_request.json
```

This avoids shell escaping issues that break inline JSON.
