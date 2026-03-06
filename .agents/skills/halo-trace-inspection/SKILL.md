# Skill: halo-trace-inspection

> **Trigger:** HALO trace, inspect trace, trace events, session events, trace keys, audit trail, verify trace, trace query
> **Category:** observability
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Guide for querying and verifying HALO trace data — the tamper-evident audit trail of all agent activity. Covers key schemas, session inspection, event retrieval, and integrity verification.

---

## HALO Trace Architecture

Every orchestrated agent task (when `trace: true`) writes events to the NucleusDB trace store:

```
PTY output → TraceBridge → StreamAdapter (parse) → TraceWriter → NucleusDB (key-value store)
```

- **TraceBridge** collects raw PTY bytes, splits into lines, feeds to the adapter
- **StreamAdapter** (ClaudeAdapter, CodexAdapter, GeminiAdapter, GenericAdapter) parses provider-specific output into structured `TraceEvent`s
- **TraceWriter** persists events with content-addressed hashing and sequential indexing

---

## Key Schema

All trace data uses structured key prefixes:

### Session Metadata
```
halo:session:<session_id>:chunk:0    → SessionMetadata JSON (agent, model, status, timestamps)
```

### Events (sequential)
```
halo:event:<session_id>:<seq>:chunk:0    → TraceEvent JSON
halo:event:<session_id>:<seq>:chunk:1    → (continuation if event exceeds chunk size)
```

`<seq>` is zero-padded (e.g., `00000001`, `00000002`).

### Indexes
```
halo:idx:agent:<agent_type>:<timestamp>:<session_id>    → ""  (presence key)
halo:idx:date:<YYYY-MM-DD>:<timestamp>:<session_id>     → ""  (presence key)
halo:idx:model:<model_name>:<timestamp>:<session_id>     → ""  (presence key)
```

### Cost Aggregation
```
halo:costs:daily:<YYYY-MM-DD>:input_tokens     → cumulative count
halo:costs:daily:<YYYY-MM-DD>:output_tokens    → cumulative count
halo:costs:daily:<YYYY-MM-DD>:cost_usd         → cumulative float
halo:costs:monthly:<YYYY-MM>:input_tokens      → cumulative count
halo:costs:monthly:<YYYY-MM>:output_tokens     → cumulative count
halo:costs:monthly:<YYYY-MM>:cost_usd          → cumulative float
```

---

## TraceEvent Structure

```json
{
  "seq": 1,
  "timestamp": 1772765810,
  "event_type": "AssistantMessage",
  "content": {"text": "I'll start by reading the file..."},
  "input_tokens": 1500,
  "output_tokens": 200,
  "cache_read_tokens": null,
  "tool_name": null,
  "tool_input": null,
  "tool_output": null,
  "file_path": null,
  "content_hash": "sha256:abcdef..."
}
```

### Event Types

| EventType | Meaning |
|-----------|---------|
| `AssistantMessage` | Agent's text response |
| `UserMessage` | Prompt or follow-up |
| `McpToolCall` | Agent invoked a tool (tool_name + tool_input populated) |
| `McpToolResult` | Tool returned a result (tool_output populated) |
| `FileChange` | Agent read/wrote a file (file_path populated) |
| `BashCommand` | Agent executed a shell command |
| `Error` | Error output (stderr) |
| `Raw` | Unparsed line (adapter couldn't classify it) |

---

## Querying Traces

### Via MCP Tools

```json
// List all sessions
{"method": "tools/call", "params": {"name": "halo_sessions", "arguments": {}}}

// Get session details
{"method": "tools/call", "params": {"name": "halo_session_detail", "arguments": {"session_id": "orch-trace-task-123"}}}

// Get events for a session
{"method": "tools/call", "params": {"name": "halo_session_events", "arguments": {"session_id": "orch-trace-task-123"}}}
```

### Via Dashboard API

```bash
# List sessions
curl http://localhost:3100/api/sessions

# Session events
curl http://localhost:3100/api/sessions/orch-trace-task-123/events

# Export session as JSON
curl http://localhost:3100/api/sessions/orch-trace-task-123/export
```

### Via Direct NucleusDB Key Query

```bash
# Count all trace keys
nucleusdb --db /tmp/experiment.ndb --command "SELECT COUNT(*) FROM kv WHERE key LIKE 'halo:%'"

# List session IDs
nucleusdb --db /tmp/experiment.ndb --command "SELECT DISTINCT key FROM kv WHERE key LIKE 'halo:session:%'"

# Get session metadata
nucleusdb --db /tmp/experiment.ndb --command "SELECT value FROM kv WHERE key = 'halo:session:orch-trace-task-123:chunk:0'"
```

---

## Verifying Trace Integrity

### Content Hash Verification

Each event's `content_hash` is `sha256(canonical_json(content))`. To verify:

```python
import hashlib, json

event = json.loads(event_json)
computed = "sha256:" + hashlib.sha256(
    json.dumps(event["content"], sort_keys=True).encode()
).hexdigest()
assert computed == event["content_hash"]
```

### Session Attestation

```bash
# Create attestation (Merkle root of all events)
curl -X POST http://localhost:3100/api/sessions/orch-trace-task-123/attest

# Verify attestation
curl -X POST http://localhost:3100/api/attestations/verify \
  -H "Content-Type: application/json" \
  -d '{"attestation_id": "att-..."}'
```

---

## Orchestrator Trace Sessions

When the orchestrator runs a task with `trace: true`, the trace session ID follows the pattern:

```
orch-trace-<task_id>
```

For example, task `task-1772765810-25076429` produces trace session `orch-trace-task-1772765810-25076429`.

This is returned in the task response's `trace_session_id` field.

---

## TraceWriter Isolation Note

`TraceWriter` creates its own `NucleusDb` instance at the configured DB path. This means:
- Trace writes go directly to disk (WAL + snapshot)
- The MCP service's in-memory SQL state does NOT see trace writes until reload
- For real-time trace inspection, use the HALO trace APIs (which read from disk), not SQL queries against the MCP service's in-memory state
