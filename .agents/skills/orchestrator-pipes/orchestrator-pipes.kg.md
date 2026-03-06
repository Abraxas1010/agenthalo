# Knowledge Graph: orchestrator-pipes

## Metadata
- domain: orchestrator
- version: 1.0.0
- skill-ref: .agents/skills/orchestrator-pipes/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Task DAG | Concept | Directed acyclic graph of tasks with edges and transforms |
| PipeTransform | Concept | Data transformation applied to source task output before target ingestion |
| Graph Nodes Object Map | Pattern | graph.nodes is {task_id: node}, NOT an array |
| Cycle Prevention | Pattern | Agent-level reachability prevents circular dependencies |
| Task Prefix Composition | Pattern | transform output prepended to target task prompt |
| Followup Dispatch | Pattern | Completed source auto-dispatches pending edges |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| orchestrator_pipe | MCP | Creates DAG edge between tasks |
| orchestrator_graph | MCP | Returns graph snapshot (nodes object + edges array) |

### Transforms
| Entity | Type | Description |
|--------|------|-------------|
| identity | Transform | Pass source output unchanged |
| claude_answer | Transform | Extract assistant text from Claude JSON output |
| json_extract | Transform | Dot-notation path extraction (e.g., "data.items[0].name") |
| prefix | Transform | Prepend static text to output |
| suffix | Transform | Append static text to output |

## Relationships
- orchestrator_pipe CREATES edge in Task DAG
- PipeTransform MODIFIES source output before target
- claude_answer RESOLVES Claude JSON array parsing
- json_extract SUPPORTS dot-notation and array indices
- Graph Nodes Object Map PREVENTS iteration errors
- Cycle Prevention BLOCKS circular DAG edges
- Followup Dispatch ENABLES automatic pipeline execution

## Cross-References
- Related skills: orchestrator-quickstart, agent-lifecycle
- CREDO imperatives served: III (Optimize — compose agents efficiently)
