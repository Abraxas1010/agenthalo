#!/usr/bin/env python3
"""Minimal MCP Streamable HTTP client for local automation and audits.

This helper handles:
1) Streamable-HTTP Accept header requirements
2) initialize -> session-id capture
3) tools/list and tools/call RPCs using the captured session-id
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = "2025-03-26"
ACCEPT_HEADER = "application/json, text/event-stream"


def _load_session_id(args: argparse.Namespace) -> str:
    if args.session_id:
        return args.session_id
    if args.session_file:
        content = Path(args.session_file).read_text(encoding="utf-8").strip()
        if content:
            return content
    raise SystemExit("missing session id: pass --session-id or --session-file")


def _save_session_id(path: str, session_id: str) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(session_id + "\n", encoding="utf-8")


def _parse_sse_json_objects(body_text: str) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    current_data: list[str] = []
    for raw_line in body_text.splitlines():
        line = raw_line.rstrip("\r")
        if not line:
            if current_data:
                blob = "\n".join(current_data).strip()
                current_data = []
                if not blob:
                    continue
                try:
                    parsed = json.loads(blob)
                except json.JSONDecodeError:
                    continue
                if isinstance(parsed, dict):
                    out.append(parsed)
            continue
        if line.startswith("data:"):
            current_data.append(line[5:].lstrip())
    if current_data:
        blob = "\n".join(current_data).strip()
        if blob:
            try:
                parsed = json.loads(blob)
            except json.JSONDecodeError:
                parsed = None
            if isinstance(parsed, dict):
                out.append(parsed)
    return out


def _post_rpc(
    endpoint: str,
    payload: dict[str, Any],
    session_id: str | None,
) -> tuple[dict[str, Any], str | None]:
    data = json.dumps(payload).encode("utf-8")
    headers = {
        "content-type": "application/json",
        "accept": ACCEPT_HEADER,
    }
    if session_id:
        headers["mcp-session-id"] = session_id
    req = urllib.request.Request(endpoint, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            body = resp.read().decode("utf-8", errors="replace")
            response_session = resp.headers.get("mcp-session-id")
            content_type = resp.headers.get("content-type", "")
    except urllib.error.HTTPError as err:
        payload_text = err.read().decode("utf-8", errors="replace")
        raise SystemExit(f"HTTP {err.code}: {payload_text}") from err
    except urllib.error.URLError as err:
        raise SystemExit(f"request failed: {err}") from err

    if "text/event-stream" in content_type:
        events = _parse_sse_json_objects(body)
        if not events:
            raise SystemExit("no JSON payload found in SSE response")
        return events[-1], response_session

    try:
        parsed = json.loads(body)
    except json.JSONDecodeError as err:
        raise SystemExit(f"non-JSON response: {body}") from err
    if not isinstance(parsed, dict):
        raise SystemExit("unexpected non-object JSON response")
    return parsed, response_session


def _post_notification(endpoint: str, payload: dict[str, Any], session_id: str) -> None:
    data = json.dumps(payload).encode("utf-8")
    headers = {
        "content-type": "application/json",
        "accept": ACCEPT_HEADER,
        "mcp-session-id": session_id,
    }
    req = urllib.request.Request(endpoint, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            _ = resp.read()
    except urllib.error.HTTPError as err:
        payload_text = err.read().decode("utf-8", errors="replace")
        raise SystemExit(f"HTTP {err.code} on notification: {payload_text}") from err
    except urllib.error.URLError as err:
        raise SystemExit(f"notification failed: {err}") from err


def cmd_init(args: argparse.Namespace) -> int:
    request = {
        "jsonrpc": "2.0",
        "id": args.request_id,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": args.client_name,
                "version": args.client_version,
            },
        },
    }
    response, session_id = _post_rpc(args.endpoint, request, None)
    if not session_id:
        raise SystemExit("initialize did not return mcp-session-id header")
    _post_notification(
        args.endpoint,
        {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}},
        session_id,
    )
    if args.session_file:
        _save_session_id(args.session_file, session_id)
    out = {"session_id": session_id, "response": response}
    print(json.dumps(out, indent=2))
    return 0


def cmd_tools_list(args: argparse.Namespace) -> int:
    session_id = _load_session_id(args)
    request = {
        "jsonrpc": "2.0",
        "id": args.request_id,
        "method": "tools/list",
        "params": {},
    }
    response, _ = _post_rpc(args.endpoint, request, session_id)
    print(json.dumps(response, indent=2))
    return 0


def cmd_tools_call(args: argparse.Namespace) -> int:
    session_id = _load_session_id(args)
    arguments: dict[str, Any] = {}
    if args.arguments:
        try:
            parsed = json.loads(args.arguments)
        except json.JSONDecodeError as err:
            raise SystemExit(f"--arguments must be valid JSON object: {err}") from err
        if not isinstance(parsed, dict):
            raise SystemExit("--arguments must decode to a JSON object")
        arguments = parsed

    request = {
        "jsonrpc": "2.0",
        "id": args.request_id,
        "method": "tools/call",
        "params": {
            "name": args.tool,
            "arguments": arguments,
        },
    }
    response, _ = _post_rpc(args.endpoint, request, session_id)
    print(json.dumps(response, indent=2))
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="MCP Streamable HTTP helper (initialize/tools/list/tools/call)."
    )
    parser.add_argument(
        "--endpoint",
        required=True,
        help="MCP endpoint URL (e.g. http://127.0.0.1:9876/mcp)",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    init = sub.add_parser("init", help="Run initialize and persist session-id.")
    init.add_argument("--session-file", help="Path to write mcp-session-id.")
    init.add_argument("--client-name", default="nucleusdb-script")
    init.add_argument("--client-version", default="1.0")
    init.add_argument("--request-id", type=int, default=1)
    init.set_defaults(func=cmd_init)

    tools_list = sub.add_parser("tools-list", help="Call tools/list.")
    tools_list.add_argument("--session-id")
    tools_list.add_argument("--session-file")
    tools_list.add_argument("--request-id", type=int, default=2)
    tools_list.set_defaults(func=cmd_tools_list)

    tools_call = sub.add_parser("tools-call", help="Call tools/call.")
    tools_call.add_argument("--session-id")
    tools_call.add_argument("--session-file")
    tools_call.add_argument("--tool", required=True)
    tools_call.add_argument(
        "--arguments",
        help="JSON object passed as tool arguments (default: {}).",
    )
    tools_call.add_argument("--request-id", type=int, default=3)
    tools_call.set_defaults(func=cmd_tools_call)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
