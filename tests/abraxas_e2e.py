#!/usr/bin/env python3
"""Abraxas VCS E2E over NucleusDB MCP Streamable HTTP.

Requires a running nucleusdb-mcp HTTP server (no auth or provide token).
"""

import json
import os
import sys
import urllib.error
import urllib.request

BASE_URL = os.environ.get("NUCLEUSDB_MCP_URL", "http://127.0.0.1:3392")
MCP_ENDPOINT = f"{BASE_URL}/mcp"
AUTH_BEARER = os.environ.get("NUCLEUSDB_MCP_AUTH_BEARER", "").strip()


def parse_sse_events(raw: str) -> list[dict]:
    events = []
    for block in raw.split("\n\n"):
        for line in block.strip().split("\n"):
            if line.startswith("data: "):
                data = line[6:].strip()
                if not data:
                    continue
                try:
                    events.append(json.loads(data))
                except json.JSONDecodeError:
                    pass
    return events


def mcp_request(method: str, params: dict, req_id: int, session_id: str | None = None):
    body = json.dumps({"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}).encode()
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if AUTH_BEARER:
        headers["Authorization"] = f"Bearer {AUTH_BEARER}"
    if session_id:
        headers["Mcp-Session-Id"] = session_id

    req = urllib.request.Request(MCP_ENDPOINT, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=15) as resp:
        return resp.read().decode(), resp.headers.get("Mcp-Session-Id", session_id or "")


def tool_call(session_id: str, name: str, arguments: dict, req_id: int):
    raw, _ = mcp_request("tools/call", {"name": name, "arguments": arguments}, req_id, session_id)
    events = parse_sse_events(raw)
    if not events:
        raise AssertionError(f"no events for tool {name}: {raw[:200]}")
    event = events[0]
    if "error" in event:
        raise AssertionError(f"tool {name} error: {event['error']}")
    content = event.get("result", {}).get("content", [])
    if not content:
        return {}
    return json.loads(content[0].get("text", "{}"))


def main() -> int:
    raw, sid = mcp_request(
        "initialize",
        {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "abraxas-e2e", "version": "1.0"},
        },
        1,
    )
    events = parse_sse_events(raw)
    if not events:
        raise AssertionError("initialize returned no events")

    try:
        mcp_request("notifications/initialized", {}, None, sid)
    except Exception:
        pass

    raw, _ = mcp_request("tools/list", {}, 2, sid)
    tools = parse_sse_events(raw)[0].get("result", {}).get("tools", [])
    names = {t["name"] for t in tools}
    for required in {"abraxas_submit_record", "abraxas_query_records", "abraxas_record_status"}:
        if required not in names:
            raise AssertionError(f"missing required Abraxas tool: {required}")

    authors = [
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ]
    base_hash = "1111111111111111111111111111111111111111111111111111111111111111"

    for i in range(10):
        rec = {
            "parents": [],
            "author_puf": authors[i % 2],
            "timestamp": 1_700_100_000 + i,
            "op": {
                "kind": "create",
                "path": f"src/file_{i}.rs",
                "content_hash": base_hash,
            },
        }
        res = tool_call(
            sid,
            "abraxas_submit_record",
            {"record_json": json.dumps(rec, separators=(",", ":"))},
            10 + i,
        )
        if not res.get("hash") or not res.get("proof_ref"):
            raise AssertionError(f"submit response missing fields: {res}")

    status = tool_call(sid, "abraxas_record_status", {}, 40)
    if status.get("record_count", 0) < 10:
        raise AssertionError(f"expected >=10 records, got {status}")

    by_path = tool_call(
        sid,
        "abraxas_query_records",
        {"path_prefix": "src/file_", "limit": 20},
        41,
    )
    if by_path.get("count", 0) < 10:
        raise AssertionError(f"expected path query >=10, got {by_path}")

    by_author = tool_call(
        sid,
        "abraxas_query_records",
        {"author_puf": authors[0], "limit": 20},
        42,
    )
    if by_author.get("count", 0) < 5:
        raise AssertionError(f"expected author query >=5, got {by_author}")

    print("Abraxas E2E PASS")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except urllib.error.HTTPError as e:
        print(f"HTTP error: {e.code} {e.read().decode()}")
        raise SystemExit(1)
    except Exception as e:
        print(f"FAIL: {e}")
        raise SystemExit(1)
