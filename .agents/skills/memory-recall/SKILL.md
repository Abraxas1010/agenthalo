# Skill: memory-recall

> **Trigger:** vector memory, embeddings, memory recall, semantic search, nomic embed, kNN search, vector store, similarity search
> **Category:** data
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Guide for using NucleusDB's vector search capabilities for semantic memory recall — storing embeddings and retrieving similar items by cosine/L2/inner-product distance.

---

## Overview

NucleusDB provides built-in kNN vector search over stored embeddings. Vectors are stored as typed values (`Vector` type) and indexed for similarity queries.

---

## Storing Vectors

### Via MCP Tool

```json
{
  "method": "tools/call",
  "params": {
    "name": "nucleusdb_set",
    "arguments": {
      "key": "memory:conversation:2026-03-05:topic-1",
      "value": [0.123, -0.456, 0.789, ...],
      "type": "vector"
    }
  }
}
```

### Via SQL

```sql
INSERT INTO kv (key, value) VALUES ('memory:doc:readme', VECTOR('[0.1, 0.2, 0.3, ...]'));
```

### Via HTTP API

```bash
curl -X POST http://localhost:3100/api/nucleusdb/edit \
  -H "Content-Type: application/json" \
  -d '{"key": "memory:doc:readme", "value": [0.1, 0.2, 0.3], "type": "vector"}'
```

---

## Querying Similar Vectors

### Via MCP Tool

```json
{
  "method": "tools/call",
  "params": {
    "name": "nucleusdb_vector_search",
    "arguments": {
      "query": [0.12, -0.45, 0.78, ...],
      "k": 5,
      "metric": "cosine"
    }
  }
}
```

### Via HTTP API

```bash
curl -X POST http://localhost:3100/api/nucleusdb/vector-search \
  -H "Content-Type: application/json" \
  -d '{"query": [0.12, -0.45, 0.78], "k": 5, "metric": "cosine"}'
```

### Metrics

| Metric | Description | Range |
|--------|-------------|-------|
| `cosine` | Cosine similarity (1 = identical, 0 = orthogonal) | [0, 1] |
| `l2` | Euclidean distance (0 = identical) | [0, ∞) |
| `ip` | Inner product (higher = more similar) | (-∞, ∞) |

---

## Embedding Generation

NucleusDB stores and searches vectors but does NOT generate embeddings. Use an external embedding model:

### nomic-embed-text (recommended for local)

```bash
# Via Ollama
ollama pull nomic-embed-text
curl http://localhost:11434/api/embeddings \
  -d '{"model": "nomic-embed-text", "prompt": "your text here"}'
```

Dimension: 768

### OpenAI text-embedding-3-small

```bash
curl https://api.openai.com/v1/embeddings \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{"model": "text-embedding-3-small", "input": "your text here"}'
```

Dimension: 1536

---

## Memory Key Schema (recommended)

Use structured key prefixes for organized retrieval:

```
memory:conversation:<date>:<topic>      → conversation embeddings
memory:document:<path-hash>             → document embeddings
memory:decision:<id>                    → decision/conclusion embeddings
memory:fact:<domain>:<id>               → factual knowledge embeddings
memory:meta:<key>:text                  → original text (companion to vector)
```

### Pattern: Store Text + Vector Together

```python
# 1. Generate embedding
embedding = embed("The authentication module uses JWT tokens")

# 2. Store the vector
nucleusdb_set(key="memory:fact:auth:jwt", value=embedding, type="vector")

# 3. Store the original text alongside
nucleusdb_set(key="memory:meta:fact:auth:jwt:text",
              value="The authentication module uses JWT tokens",
              type="text")
```

### Pattern: Recall and Retrieve

```python
# 1. Embed the query
query_vec = embed("How does authentication work?")

# 2. Search for similar
results = nucleusdb_vector_search(query=query_vec, k=5, metric="cosine")

# 3. Retrieve original text for top results
for result in results:
    text_key = f"memory:meta:{result.key}:text"
    original = nucleusdb_get(key=text_key)
```

---

## Integration with Orchestrator

### Agent Memory Pattern

Give each orchestrated agent access to shared vector memory:

```json
{
  "agent": "claude",
  "agent_name": "researcher",
  "capabilities": ["memory_read", "memory_write"],
  "trace": true
}
```

The agent can then use `nucleusdb_set` and `nucleusdb_vector_search` MCP tools to read/write memory.

### Cross-Agent Knowledge Sharing

1. **Agent A** stores findings as embeddings
2. **Agent B** queries the same NucleusDB instance for similar knowledge
3. Both agents share the same `--db` path → same vector index

---

## Limitations

- **No automatic indexing:** Vectors are indexed on insert. Bulk inserts may be slow.
- **Brute-force kNN:** Current implementation is exact kNN (no approximate algorithms). Performance degrades above ~100K vectors.
- **Dimension must be consistent:** All vectors in a search must have the same dimensionality. Mixing 768-dim and 1536-dim vectors in the same query will fail.
- **No metadata filtering:** Vector search returns the k nearest neighbors globally. Filter by key prefix in application code.
