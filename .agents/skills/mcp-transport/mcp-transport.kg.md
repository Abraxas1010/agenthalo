# Knowledge Graph: mcp-transport

## Metadata
- domain: infrastructure
- version: 1.0.0
- skill-ref: .agents/skills/mcp-transport/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Streamable HTTP | Concept | JSON-RPC over SSE (not stdio, not WebSocket) |
| Session Lifecycle | Pattern | initialize → notifications/initialized → tools/call (mandatory sequence) |
| Mcp-Session-Id | Concept | Header for session tracking, extracted from initialize response |
| SSE Response Format | Pattern | data: lines with double-parse required |
| Double Parse | Pattern | result.content[0].text is a JSON string — parse twice |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| initialize | MCP Method | Starts session, returns protocolVersion + capabilities |
| notifications/initialized | MCP Method | Mandatory confirmation (no id, returns 202) |
| tools/call | MCP Method | Execute any MCP tool |
| tools/list | MCP Method | Enumerate available tools |
| /health | HTTP | Server health check endpoint |

## Relationships
- Session Lifecycle REQUIRES initialize BEFORE notifications/initialized BEFORE tools/call
- Mcp-Session-Id REQUIRED_BY all post-initialize requests
- SSE Response Format REQUIRES Double Parse for tool results
- Skipping notifications/initialized CAUSES session instability
- Stale Mcp-Session-Id CAUSES 401 errors

## Decision Trees

```
Session Error?
├── 401 Unauthorized
│   ├── Session expired → Re-run full initialize sequence
│   ├── Missing Mcp-Session-Id header → Add header from initialize response
│   └── Auth required → Add Authorization: Bearer header
├── 400 Bad Request
│   └── Wrong method name → Must be exactly "tools/call"
├── Empty response
│   └── Not reading full SSE stream → Use stream=True or curl -N
└── Result is string, not object
    └── Single-parsed → Double-parse: json.loads(result.content[0].text)
```

## Cross-References
- Related skills: orchestrator-quickstart, halo-trace-inspection
- CREDO imperatives served: I (Trust — verify session state), IV (Document — error patterns)
