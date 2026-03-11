#!/usr/bin/env python3
"""End-to-end test for NucleusDB remote MCP server over Streamable HTTP.

Tests:
1. Health endpoint
2. Auth info endpoint
3. MCP initialize → tools/list → tools/call (nucleusdb_help)
4. MCP initialize → tools/call (nucleusdb_status)
5. Optional auth-negative check (when NUCLEUSDB_MCP_EXPECT_AUTH=1)
"""

import json
import os
import signal
import subprocess
import sys
import time
import urllib.request
import urllib.error


BASE_URL = os.environ.get("NUCLEUSDB_MCP_URL", "http://127.0.0.1:3392")
MCP_ENDPOINT = f"{BASE_URL}/mcp"
HEALTH_ENDPOINT = f"{BASE_URL}/health"
AUTH_ENDPOINT = f"{BASE_URL}/auth/info"
AUTH_BEARER = os.environ.get("NUCLEUSDB_MCP_AUTH_BEARER", "").strip()
EXPECT_AUTH = os.environ.get("NUCLEUSDB_MCP_EXPECT_AUTH", "").strip().lower() in {"1", "true", "yes"}


def parse_sse_events(raw: str) -> list[dict]:
    """Parse SSE stream into list of JSON-RPC messages."""
    events = []
    for block in raw.split("\n\n"):
        for line in block.strip().split("\n"):
            if line.startswith("data: "):
                data = line[6:].strip()
                if data:
                    try:
                        events.append(json.loads(data))
                    except json.JSONDecodeError:
                        pass
    return events


def mcp_request(method: str, params: dict, req_id: int, session_id: str = None) -> tuple[str, str]:
    """Send an MCP JSON-RPC request and return (response_body, session_id)."""
    body = json.dumps({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    }).encode()

    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if AUTH_BEARER:
        headers["Authorization"] = f"Bearer {AUTH_BEARER}"
    if session_id:
        headers["Mcp-Session-Id"] = session_id

    req = urllib.request.Request(MCP_ENDPOINT, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=10) as resp:
        sid = resp.headers.get("Mcp-Session-Id", session_id or "")
        return resp.read().decode(), sid


def mcp_request_no_auth(method: str, params: dict, req_id: int, session_id: str = None) -> tuple[str, str]:
    """Send MCP JSON-RPC request without Authorization header."""
    body = json.dumps({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    }).encode()
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if session_id:
        headers["Mcp-Session-Id"] = session_id
    req = urllib.request.Request(MCP_ENDPOINT, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=10) as resp:
        sid = resp.headers.get("Mcp-Session-Id", session_id or "")
        return resp.read().decode(), sid


def test_health():
    """Test /health endpoint."""
    req = urllib.request.Request(HEALTH_ENDPOINT)
    with urllib.request.urlopen(req, timeout=5) as resp:
        data = json.loads(resp.read())
    assert data["status"] == "ok", f"health status: {data}"
    assert data["transport"] == "streamable-http"
    assert data["protocol"] == "mcp/2025-03-26"
    print(f"  PASS: health (version={data['version']})")


def test_auth_info():
    """Test /auth/info endpoint."""
    req = urllib.request.Request(AUTH_ENDPOINT)
    with urllib.request.urlopen(req, timeout=5) as resp:
        data = json.loads(resp.read())
    assert "methods" in data
    assert "cab" in data["methods"]
    assert "oauth" in data["methods"]
    assert "tool_scopes" in data
    print(f"  PASS: auth info (enabled={data['auth_enabled']})")


def test_auth_required_rejects_missing_header():
    """When auth is enabled, unauthenticated MCP requests are rejected."""
    try:
        mcp_request_no_auth("initialize", {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "no-auth-test", "version": "1.0"},
        }, req_id=99)
        raise AssertionError("expected HTTP 401 but request succeeded")
    except urllib.error.HTTPError as e:
        assert e.code == 401, f"expected 401, got {e.code}"
        body = e.read().decode()
        assert "Authorization" in body or "Bearer" in body
    print("  PASS: auth-required rejects missing Authorization header")


def test_mcp_initialize_and_tools():
    """Test MCP session: initialize → initialized → tools/list → tools/call."""
    # Step 1: Initialize
    raw, session_id = mcp_request("initialize", {
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": {"name": "e2e-test", "version": "1.0"},
    }, req_id=1)

    events = parse_sse_events(raw)
    assert len(events) >= 1, f"expected initialize response, got: {raw[:200]}"
    result = events[0].get("result", {})
    assert result.get("protocolVersion") == "2025-03-26", f"protocol: {result}"
    assert result.get("serverInfo", {}).get("name") == "nucleusdb"
    assert "tools" in result.get("capabilities", {}), f"missing tools capability"
    print(f"  PASS: initialize (session={session_id[:16] if session_id else 'none'}...)")

    # Step 2: Send initialized notification
    if session_id:
        try:
            mcp_request("notifications/initialized", {}, req_id=None, session_id=session_id)
        except Exception:
            pass  # Notifications don't always return a response

    # Step 3: List tools
    if session_id:
        raw, _ = mcp_request("tools/list", {}, req_id=2, session_id=session_id)
        events = parse_sse_events(raw)
        assert len(events) >= 1, f"expected tools list, got: {raw[:200]}"
        tools = events[0].get("result", {}).get("tools", [])
        tool_names = [t["name"] for t in tools]
        print(f"  PASS: tools/list ({len(tools)} tools)")

        # Verify key tools are present
        expected_tools = [
            "nucleusdb_help", "nucleusdb_status", "nucleusdb_execute_sql",
            "nucleusdb_verify_agent", "verify_agent_multichain",
            "nucleusdb_agent_register", "register_chain",
        ]
        for name in expected_tools:
            assert name in tool_names, f"missing tool: {name}"
        print(f"  PASS: all {len(expected_tools)} key tools present")

        # Step 4: Call nucleusdb_help
        raw, _ = mcp_request("tools/call", {
            "name": "nucleusdb_help",
            "arguments": {},
        }, req_id=3, session_id=session_id)
        events = parse_sse_events(raw)
        assert len(events) >= 1, f"expected help response, got: {raw[:200]}"
        content = events[0].get("result", {}).get("content", [])
        assert len(content) > 0, "help returned empty content"
        help_text = content[0].get("text", "")
        help_data = json.loads(help_text)
        assert help_data.get("server") == "nucleusdb"
        assert "binary_merkle" in str(help_data.get("backends", []))
        print(f"  PASS: tools/call nucleusdb_help")

        # Step 5: Call nucleusdb_status
        raw, _ = mcp_request("tools/call", {
            "name": "nucleusdb_status",
            "arguments": {},
        }, req_id=4, session_id=session_id)
        events = parse_sse_events(raw)
        assert len(events) >= 1, f"expected status response, got: {raw[:200]}"
        content = events[0].get("result", {}).get("content", [])
        assert len(content) > 0
        status_data = json.loads(content[0].get("text", "{}"))
        assert status_data.get("backend") == "binary_merkle"
        print(f"  PASS: tools/call nucleusdb_status (backend={status_data['backend']})")
    else:
        print("  SKIP: tools/list and tools/call (no session ID returned)")


def test_mcp_execute_sql():
    """Test MCP session: initialize → execute SQL → query → verify."""
    raw, session_id = mcp_request("initialize", {
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": {"name": "sql-test", "version": "1.0"},
    }, req_id=1)

    if not session_id:
        print("  SKIP: SQL test (no session)")
        return

    # Send initialized
    try:
        mcp_request("notifications/initialized", {}, req_id=None, session_id=session_id)
    except Exception:
        pass

    # INSERT + COMMIT
    raw, _ = mcp_request("tools/call", {
        "name": "nucleusdb_execute_sql",
        "arguments": {"sql": "INSERT INTO data (key, value) VALUES ('test:1', 42); COMMIT;"},
    }, req_id=2, session_id=session_id)
    events = parse_sse_events(raw)
    assert len(events) >= 1
    result_text = events[0].get("result", {}).get("content", [{}])[0].get("text", "")
    result = json.loads(result_text)
    assert result.get("status") in ("ok", "rows"), f"SQL failed: {result}"
    print(f"  PASS: execute_sql INSERT+COMMIT")

    # QUERY
    raw, _ = mcp_request("tools/call", {
        "name": "nucleusdb_query",
        "arguments": {"key": "test:1"},
    }, req_id=3, session_id=session_id)
    events = parse_sse_events(raw)
    assert len(events) >= 1
    query_text = events[0].get("result", {}).get("content", [{}])[0].get("text", "")
    query_result = json.loads(query_text)
    assert query_result.get("value") == 42, f"wrong value: {query_result}"
    assert query_result.get("verified") == True, f"not verified: {query_result}"
    print(f"  PASS: query test:1 (value={query_result['value']}, verified={query_result['verified']})")


def main():
    passed = 0
    failed = 0
    tests = [
        ("Health endpoint", test_health),
        ("Auth info endpoint", test_auth_info),
        ("MCP initialize + tools", test_mcp_initialize_and_tools),
        ("MCP SQL roundtrip", test_mcp_execute_sql),
    ]
    if EXPECT_AUTH:
        tests.append(("Auth required (no header)", test_auth_required_rejects_missing_header))

    for name, fn in tests:
        print(f"\n[TEST] {name}")
        try:
            fn()
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

    print(f"\n{'='*60}")
    print(f"Results: {passed} passed, {failed} failed, {len(tests)} total")
    sys.exit(1 if failed > 0 else 0)


if __name__ == "__main__":
    main()
