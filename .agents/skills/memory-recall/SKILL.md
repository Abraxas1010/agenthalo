# Skill: memory-recall

> **Trigger:** vector memory, embeddings, memory recall, semantic search, memory store, similarity search, remember, forget, context recall, agent memory
> **Category:** data
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Guide for using NucleusDB's semantic memory system — storing text memories with automatic embedding, recalling them via natural-language queries, and ingesting documents as chunked memory fragments.

---

## Quick Start (MCP Tools)

Three high-level memory tools handle embedding, storage, and retrieval automatically:

### Store a Memory

```json
{
  "name": "agenthalo_memory_store",
  "arguments": {
    "text": "The authentication module uses JWT tokens with 24h expiry",
    "source": "session:design-review",
    "session_id": "sess-2026-03-20",
    "agent_id": "claude",
    "ttl_secs": null
  }
}
```

- `text` (required): The memory content to store and embed
- `source` (optional): Label for tracking origin (e.g. `session:id`, `user:note`, `auto:tool_name`)
- `session_id` (optional): Session context — enriches the embedding for better retrieval
- `agent_id` (optional): Agent identity context
- `ttl_secs` (optional): Time-to-live in seconds. `null` = permanent. Auto-captured memories default to 604800 (7 days).

Returns: `key`, `created`, `dims`, `sealed`

### Recall Memories

```json
{
  "name": "agenthalo_memory_recall",
  "arguments": {
    "query": "how does authentication work in the system",
    "k": 5
  }
}
```

- `query` (required): Natural-language question describing what you need
- `k` (optional): Number of results to return (1-20, default 5)

Returns ranked results with `key`, `text`, `distance`, `source`, `created`.

The recall pipeline automatically:
1. Expands the query using HyDE (hypothetical document expansion)
2. Embeds with Nomic Embed Text v1.5 (768-dim, ONNX)
3. Searches memory vectors with prefix-filtered cosine similarity
4. Reranks using fused scoring: base similarity (50%) + asymmetric bi-encoder (28%) + lexical overlap (12%) + negation alignment (10%)
5. Filters out TTL-expired memories

### Ingest a Document

```json
{
  "name": "agenthalo_memory_ingest",
  "arguments": {
    "document": "## Architecture\nThe system uses...\n\n## Security\nAuthentication is...",
    "source": "doc:architecture.md"
  }
}
```

Splits by markdown headings, chunks at ~512 words, prepends section headings to each chunk for retrieval context, then stores each chunk as a separate memory.

---

## Recall Quality Features

### Query Expansion (HyDE)
Bridges vocabulary gaps. "mathematical guarantees" expands to include "formal verification, machine-checked proofs". Covers 13 domain vocabularies: security, agent/session, network, identity, trust, payment, vector, lean/theorem, proof, software, privacy, negation.

### Negation-Aware Reranking
Query "endpoint NOT private" correctly ranks documents containing "not private" above documents about "private endpoints".

### Section-Aware Chunks
Document chunks include their heading context: `[Section: Security Model] The authentication uses...` — so retrieval matches are contextualized.

### TTL-Based Expiry
Memories with `ttl_secs` are excluded from recall after expiry. Auto-captured tool results default to 7-day TTL.

---

## Auto-Capture (F1)

When enabled (`NUCLEUSDB_AUTO_CAPTURE=true`), tool call results are automatically stored as memories with:
- Source: `auto:{tool_name}`
- TTL: 7 days
- Format: `[Tool: {name}] {summary}`

---

## Embedding Model

- **Model**: Nomic Embed Text v1.5 (ONNX runtime)
- **Dimensions**: 768
- **Task prefixes**: `search_document:` for storage, `search_query:` for recall
- **Normalization**: L2-normalized vectors
- **Fallback**: SHA256-based deterministic hash backend for testing (`NUCLEUSDB_EMBEDDING_BACKEND=hash-test`)
- **Model location**: `$NOMIC_MODEL_DIR` or `~/.nucleusdb/models/nomic-embed-text/` (needs `model.onnx` + `tokenizer.json`)

---

## Low-Level Access

For advanced use cases, the raw vector operations are still available:

| MCP Tool | Description |
|----------|-------------|
| `nucleusdb_set` (type: "vector") | Store raw vector by key |
| `nucleusdb_vector_search` | Raw kNN search with metric selection |
| `nucleusdb_get` | Retrieve stored value by key |

Metrics: `cosine` (default), `l2`, `ip`

---

## Limitations

- **Brute-force kNN with prefix filter**: Exact search over matching key prefix. Fast up to ~100K vectors. Uses BTreeMap range scan to skip non-matching keys.
- **Dimension consistency**: All vectors in a search must have the same dimensionality (768 for nomic).
- **ONNX model required for production**: The hash-test backend is deterministic but not semantically meaningful. Real semantic similarity requires the ONNX model files.
