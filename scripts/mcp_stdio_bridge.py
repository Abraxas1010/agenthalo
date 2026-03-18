#!/usr/bin/env python3
"""MCP stdio-to-HTTP bridge for Agent H.A.L.O.

Bridges Claude Code / Codex / Gemini's stdio-based MCP protocol to the
agenthalo-mcp-server HTTP JSON-RPC endpoint.  Reads JSON-RPC messages
from stdin (one per line), POSTs them to http://127.0.0.1:<port>/mcp,
and writes the JSON-RPC response to stdout.

Auto-discovery: if AGENTHALO_MCP_PORT/SECRET are not set, scans
running agenthalo-mcp-server processes to extract their port and secret.

Environment variables:
  AGENTHALO_MCP_PORT   – HTTP server port (auto-discovered if unset)
  AGENTHALO_MCP_SECRET – Bearer token for auth (auto-discovered if unset)
  AGENTHALO_ALLOW_DEV_SECRET – if "1", use dev fallback secret
  AGENTHALO_MCP_BRIDGE_AUTO_START – if "1", launch mcp-server if not running
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

# Find the binary relative to this script
SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
BINARY = REPO_ROOT / "target" / "release" / "agenthalo-mcp-server"

_server_proc: subprocess.Popen | None = None
_port: str = ""
_secret: str = ""


def _discover_running_server() -> tuple[str, str]:
    """Scan running agenthalo-mcp-server processes for port and secret."""
    try:
        result = subprocess.run(
            ["pgrep", "-a", "-f", "agenthalo-mcp-server"],
            capture_output=True, text=True, timeout=5,
        )
        if result.returncode != 0:
            return "", ""
    except Exception:
        return "", ""

    for line in result.stdout.strip().splitlines():
        parts = line.split(None, 1)
        if len(parts) < 2:
            continue
        pid_str = parts[0]
        try:
            pid = int(pid_str)
        except ValueError:
            continue

        # Read environment from /proc/<pid>/environ
        try:
            env_bytes = Path(f"/proc/{pid}/environ").read_bytes()
            env_entries = env_bytes.split(b"\x00")
            env_map = {}
            for entry in env_entries:
                decoded = entry.decode("utf-8", errors="replace")
                if "=" in decoded:
                    k, v = decoded.split("=", 1)
                    env_map[k] = v

            port = env_map.get("AGENTHALO_MCP_PORT") or env_map.get("NUCLEUSDB_MCP_PORT", "")
            secret = env_map.get("AGENTHALO_MCP_SECRET", "")
            if port:
                _log(f"discovered server on pid {pid}: port={port}")
                return port, secret
        except (PermissionError, FileNotFoundError):
            continue

    return "", ""


def _resolve_config() -> tuple[str, str]:
    """Resolve port and secret from env vars or auto-discovery."""
    port = os.environ.get("AGENTHALO_MCP_PORT", "")
    secret = os.environ.get("AGENTHALO_MCP_SECRET", "")

    if not port:
        disc_port, disc_secret = _discover_running_server()
        if disc_port:
            port = disc_port
            if not secret:
                secret = disc_secret

    if not port:
        port = "8390"  # default

    if not secret and os.environ.get("AGENTHALO_ALLOW_DEV_SECRET"):
        secret = "agenthalo-dev-secret"

    return port, secret


def _is_server_running(port: str) -> bool:
    """Check if the MCP server is responding on the expected port."""
    try:
        req = urllib.request.Request(
            f"http://127.0.0.1:{port}/health", method="GET",
        )
        with urllib.request.urlopen(req, timeout=2) as resp:
            return resp.status == 200
    except Exception:
        return False


def _start_server(port: str) -> None:
    """Launch agenthalo-mcp-server as a background process."""
    global _server_proc
    if not BINARY.exists():
        _log(f"binary not found: {BINARY}")
        return
    env = dict(os.environ)
    env["AGENTHALO_MCP_PORT"] = port
    if not env.get("AGENTHALO_MCP_SECRET") and not env.get("AGENTHALO_ALLOW_DEV_SECRET"):
        env["AGENTHALO_ALLOW_DEV_SECRET"] = "1"
    _server_proc = subprocess.Popen(
        [str(BINARY)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    for _ in range(30):
        time.sleep(0.5)
        if _is_server_running(port):
            _log("mcp-server started")
            return
    _log("mcp-server did not become healthy within 15s")


def _cleanup(*_args):
    """Kill server subprocess on exit."""
    if _server_proc and _server_proc.poll() is None:
        _server_proc.terminate()
        try:
            _server_proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            _server_proc.kill()
    sys.exit(0)


def _log(msg: str) -> None:
    """Log to stderr (never stdout — that's the MCP channel)."""
    print(f"[mcp-bridge] {msg}", file=sys.stderr, flush=True)


def _post_rpc(endpoint: str, secret: str, payload: dict) -> dict:
    """POST a JSON-RPC request to the HTTP MCP server."""
    data = json.dumps(payload).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json",
    }
    if secret:
        headers["Authorization"] = f"Bearer {secret}"

    req = urllib.request.Request(endpoint, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            body = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as err:
        body = err.read().decode("utf-8", errors="replace")
        return {
            "jsonrpc": "2.0",
            "id": payload.get("id"),
            "error": {
                "code": -32000,
                "message": f"HTTP {err.code}: {body[:500]}",
            },
        }
    except urllib.error.URLError as err:
        return {
            "jsonrpc": "2.0",
            "id": payload.get("id"),
            "error": {
                "code": -32000,
                "message": f"Connection failed: {err}",
            },
        }

    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        return {
            "jsonrpc": "2.0",
            "id": payload.get("id"),
            "error": {
                "code": -32000,
                "message": f"Non-JSON response: {body[:500]}",
            },
        }

    # Wrap in JSON-RPC envelope if the server returned a raw result
    if isinstance(parsed, dict) and "jsonrpc" not in parsed:
        return {
            "jsonrpc": "2.0",
            "id": payload.get("id"),
            "result": parsed,
        }
    return parsed


def main() -> int:
    signal.signal(signal.SIGTERM, _cleanup)
    signal.signal(signal.SIGINT, _cleanup)

    port, secret = _resolve_config()
    endpoint = f"http://127.0.0.1:{port}/mcp"
    auto_start = os.environ.get("AGENTHALO_MCP_BRIDGE_AUTO_START", "0") == "1"

    # Ensure server is running
    if not _is_server_running(port):
        if auto_start:
            _log("server not running, starting...")
            _start_server(port)
        if not _is_server_running(port):
            _log(f"server not reachable at {endpoint}")

    _log(f"bridge ready, forwarding to {endpoint}")

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            _log(f"invalid JSON: {line[:200]}")
            continue

        # Handle notifications (no response expected)
        if "id" not in request:
            _post_rpc(endpoint, secret, request)
            continue

        response = _post_rpc(endpoint, secret, request)
        sys.stdout.write(json.dumps(response) + "\n")
        sys.stdout.flush()

    return 0


if __name__ == "__main__":
    sys.exit(main())
