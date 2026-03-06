# Knowledge Graph: memory-recall

## Metadata
- domain: data
- version: 1.0.0
- skill-ref: .agents/skills/memory-recall/SKILL.md
- credo-ref: .agents/CREDO.md

## Entities

### Concepts
| Entity | Type | Description |
|--------|------|-------------|
| Vector Storage | Concept | Typed Vector values stored via key-value interface |
| kNN Search | Pattern | Exact nearest-neighbor search (brute-force, no ANN) |
| Text+Vector Pattern | Pattern | Store embedding at key, store original text at meta: companion key |
| Memory Key Schema | Pattern | Structured prefixes: memory:conversation:, memory:document:, etc. |
| Cross-Agent Memory | Pattern | Multiple agents sharing same NucleusDB instance for knowledge transfer |

### Metrics
| Entity | Type | Description |
|--------|------|-------------|
| cosine | Metric | Cosine similarity [0, 1] — 1 = identical |
| l2 | Metric | Euclidean distance [0, ∞) — 0 = identical |
| ip | Metric | Inner product (-∞, ∞) — higher = more similar |

### Tools
| Entity | Type | Integration |
|--------|------|-------------|
| nucleusdb_set | MCP | Store vector with type: "vector" |
| nucleusdb_vector_search | MCP | kNN query with k, metric params |
| nucleusdb_get | MCP | Retrieve companion text by key |
| /api/nucleusdb/edit | HTTP | Store vector via HTTP API |
| /api/nucleusdb/vector-search | HTTP | kNN query via HTTP API |

### Embedding Models
| Entity | Type | Description |
|--------|------|-------------|
| nomic-embed-text | Model | Local via Ollama, 768 dimensions |
| text-embedding-3-small | Model | OpenAI API, 1536 dimensions |

## Relationships
- Vector Storage STORES embeddings as typed values
- kNN Search QUERIES stored vectors by similarity
- Text+Vector Pattern ENABLES retrieval of original text for matches
- Memory Key Schema ORGANIZES memory by category and purpose
- Cross-Agent Memory ENABLES knowledge sharing between orchestrated agents
- cosine RECOMMENDED for normalized embeddings
- Dimension consistency REQUIRED across all vectors in a search

## Limitations
- Brute-force kNN — degrades above ~100K vectors
- No metadata filtering — filter by key prefix in application code
- Dimension must be consistent within a query

## Cross-References
- Related skills: orchestrator-quickstart (agent memory pattern)
- CREDO imperatives served: II (Search — semantic recall), V (Collaborate — shared memory)
