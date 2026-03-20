# Skill: library-search

> **Trigger:** past session, previous work, what happened before, cross-session, library search, history search, prior agent work, accumulated knowledge, session history, what did the last agent do, previous decision, recall past, earlier session, find session, knowledge base search, search all sessions
> **Category:** data
> **Audience:** Internal (hardwired) + External (controlling agent)

## Purpose

Search the persistent Library for knowledge accumulated across all past agent sessions. Supports both semantic (vector) search and keyword search.

---

## When To Use

- "What did we do about X last time?" → `library_semantic_search`
- "Find previous sessions about authentication" → `library_semantic_search`
- "Search all past agent work for payment channels" → `library_semantic_search`
- Keyword-exact matches → `library_search`
- Browse by key prefix → `library_browse`

## Semantic Search (recommended)

```json
{
  "name": "library_semantic_search",
  "arguments": {
    "query": "how did the previous agent handle JWT token refresh",
    "k": 5
  }
}
```

Uses vector embeddings of session summaries. Understands meaning — "authentication tokens" will match sessions about "JWT validation" even without exact keyword overlap.

Returns: `key`, `distance`, `text`, `session_id`, `source`, `created`

## Keyword Search (fallback)

```json
{
  "name": "library_search",
  "arguments": {
    "query": "JWT authentication",
    "limit": 10
  }
}
```

Simple whitespace-separated term matching. Fast but requires exact keyword overlap.

## Browse

```json
{
  "name": "library_browse",
  "arguments": {
    "prefix": "lib:session:",
    "limit": 20
  }
}
```

Browse by key prefix. Use `lib:session:` for sessions, `lib:evt:` for events, `lib:summary:` for summaries.

## Session Lookup

```json
{
  "name": "library_session_lookup",
  "arguments": {
    "session_id": "orch-1774027493-edc59fe7"
  }
}
```

Get full details for a specific session: metadata, summary, event count.

---

## How It Works

The Library accumulates knowledge from all agent sessions:
1. Agent session ends → `push_session()` writes metadata, summary, events to `~/.agenthalo/library/library.ndb`
2. Session summary is automatically embedded into `library_embeddings.ndb` (semantic sidecar)
3. Future agents search the sidecar via `library_semantic_search`
4. The sidecar is regenerable — if corrupted, `nucleusdb library embed-backfill` rebuilds it

---

## Relationship to Memory Tools

| Tool | Scope | Use Case |
|------|-------|----------|
| `agenthalo_memory_store/recall` | Current session only | Store/recall within a single session |
| `library_semantic_search` | All past sessions | Cross-session semantic recall |
| `library_search` | All past sessions | Cross-session keyword search |

For comprehensive recall, use BOTH:
1. `agenthalo_memory_recall` — what was stored in this session
2. `library_semantic_search` — what happened in past sessions
