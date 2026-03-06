# Knowledge Graph: halo-trace-inspection

## Metadata
- domain: observability
- version: 1.0.0
- skill-ref: .agents/skills/halo-trace-inspection/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Trace Pipeline | Pattern | PTY → TraceBridge → StreamAdapter → TraceWriter → NucleusDB |
| TraceEvent | Concept | Structured event with seq, timestamp, event_type, content, content_hash |
| Session Metadata | Concept | Agent, model, status, timestamps stored at halo:session:<id>:chunk:0 |
| Content Hash | Pattern | sha256(canonical_json(content)) for tamper detection |
| TraceWriter Isolation | Pattern | TraceWriter has own NucleusDB instance — MCP SQL state is separate |
| Key Schema | Pattern | Structured prefixes: halo:session:, halo:event:, halo:idx:, halo:costs: |

### Event Types
| Entity | Type | Description |
|--------|------|-------------|
| AssistantMessage | EventType | Agent's text response |
| UserMessage | EventType | Prompt or follow-up |
| McpToolCall | EventType | Agent invoked a tool |
| McpToolResult | EventType | Tool returned a result |
| FileChange | EventType | Agent read/wrote a file |
| BashCommand | EventType | Agent executed a shell command |
| Error | EventType | Stderr output |
| Raw | EventType | Unparsed line |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| halo_sessions | MCP | List all trace sessions |
| halo_session_detail | MCP | Get session metadata |
| halo_session_events | MCP | Get events for a session |
| /api/sessions | HTTP | Dashboard session list |
| /api/sessions/:id/events | HTTP | Dashboard event list |
| /api/sessions/:id/export | HTTP | Export session as JSON |
| /api/sessions/:id/attest | HTTP | Create Merkle root attestation |

## Relationships
- Trace Pipeline PRODUCES TraceEvent sequence
- TraceBridge COLLECTS PTY bytes and FEEDS StreamAdapter
- StreamAdapter PARSES provider output INTO TraceEvent
- TraceWriter PERSISTS events TO NucleusDB on disk
- Content Hash ENABLES tamper detection
- TraceWriter Isolation PREVENTS SQL queries from seeing live trace writes
- Key Schema ORGANIZES all trace data under halo: prefix

## Cross-References
- Related skills: agent-lifecycle, orchestrator-quickstart, mcp-transport
- CREDO imperatives served: I (Trust — verify trace integrity), IV (Document — audit trails)
