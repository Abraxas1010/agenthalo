#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLIENT="${ROOT_DIR}/scripts/mcp_streamable_http.py"

if [[ ! -x "${CLIENT}" ]]; then
  echo "missing executable ${CLIENT}" >&2
  exit 1
fi

PORT="${PORT:-9986}"
DB_PATH="$(mktemp -u /tmp/orchestrator_smoke_XXXXXX.ndb)"
LOG_PATH="$(mktemp -u /tmp/orchestrator_smoke_XXXXXX.log)"
SESSION_FILE="$(mktemp -u /tmp/orchestrator_smoke_session_XXXXXX.txt)"

if command -v nucleusdb-mcp >/dev/null 2>&1; then
  SERVER_CMD=(nucleusdb-mcp --transport http --host 127.0.0.1 --port "${PORT}" --db "${DB_PATH}" --no-auth)
else
  SERVER_CMD=(cargo run --quiet --bin nucleusdb-mcp -- --transport http --host 127.0.0.1 --port "${PORT}" --db "${DB_PATH}" --no-auth)
fi

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" >/dev/null 2>&1 || true
  fi
  rm -f "${SESSION_FILE}"
}
trap cleanup EXIT

cd "${ROOT_DIR}"
"${SERVER_CMD[@]}" >"${LOG_PATH}" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 120); do
  if curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
if ! curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
  echo "server failed to start; see ${LOG_PATH}" >&2
  tail -n 120 "${LOG_PATH}" >&2 || true
  exit 1
fi

ENDPOINT="http://127.0.0.1:${PORT}/mcp"
python3 "${CLIENT}" --endpoint "${ENDPOINT}" init --session-file "${SESSION_FILE}" >/tmp/orch_smoke_init.json

launch_json="$(python3 "${CLIENT}" \
  --endpoint "${ENDPOINT}" \
  tools-call \
  --session-file "${SESSION_FILE}" \
  --tool orchestrator_launch \
  --arguments '{"agent":"shell","agent_name":"smoke-shell","timeout_secs":30,"trace":true,"capabilities":["memory_read"]}')"

agent_id="$(python3 - <<'PY' "${launch_json}"
import json,sys
data=json.loads(sys.argv[1])
sc=(data.get("result") or {}).get("structuredContent")
if sc is None:
    txt=((data.get("result") or {}).get("content") or [{}])[0].get("text","{}")
    try:
        sc=json.loads(txt)
    except Exception:
        sc={}
print(sc.get("agent_id",""))
PY
)"
if [[ -z "${agent_id}" ]]; then
  echo "failed to parse orchestrator agent_id" >&2
  echo "${launch_json}" >&2
  exit 1
fi

check_task() {
  local command="$1"
  local expect="$2"
  local task_json
  task_json="$(python3 "${CLIENT}" \
    --endpoint "${ENDPOINT}" \
    tools-call \
    --session-file "${SESSION_FILE}" \
    --tool orchestrator_send_task \
    --arguments "{\"agent_id\":\"${agent_id}\",\"task\":\"${command}\",\"timeout_secs\":20,\"wait\":true}")"

  python3 - <<'PY' "${task_json}" "${command}" "${expect}"
import json,sys
data=json.loads(sys.argv[1])
command=sys.argv[2]
expected=sys.argv[3]
sc=(data.get("result") or {}).get("structuredContent")
if sc is None:
    txt=((data.get("result") or {}).get("content") or [{}])[0].get("text","{}")
    try:
        sc=json.loads(txt)
    except Exception:
        sc={}
status=sc.get("status","")
trace=sc.get("trace_session_id")
result=sc.get("result","") or ""
if status != "complete":
    raise SystemExit(f"task `{command}` status={status!r}, expected complete")
if not trace:
    raise SystemExit(f"task `{command}` missing trace_session_id")
if expected and expected not in result:
    raise SystemExit(f"task `{command}` missing expected output {expected!r}")
print(f"task_ok command={command!r} trace={trace}")
PY
}

check_task "true" ""
check_task "printf 'smoke-trace-ok'" "smoke-trace-ok"
check_task "echo smoke-trace-done" "smoke-trace-done"

python3 "${CLIENT}" \
  --endpoint "${ENDPOINT}" \
  tools-call \
  --session-file "${SESSION_FILE}" \
  --tool orchestrator_stop \
  --arguments "{\"agent_id\":\"${agent_id}\",\"force\":false}" >/tmp/orch_smoke_stop.json

echo "orchestrator_mcp_smoke: PASS"
echo "server_log=${LOG_PATH}"
echo "db_path=${DB_PATH}"
