#!/usr/bin/env bash
set -euo pipefail

IMAGE_TAG="${IMAGE_TAG:-agenthalo:phase7}"
NETWORK_NAME="${NETWORK_NAME:-agenthalo-phase7-$$}"
OPERATOR_NAME="${OPERATOR_NAME:-agenthalo-phase7-operator}"
DASHBOARD_PORT="${DASHBOARD_PORT:-43100}"
MODEL_ID="${MODEL_ID:-sshleifer/tiny-gpt2}"
RUN_MODEL_PULL="${RUN_MODEL_PULL:-auto}"
BUILD_IMAGE="${BUILD_IMAGE:-1}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

OPERATOR_DATA="$(mktemp -d "${TMPDIR:-/tmp}/agenthalo-phase7-operator.XXXXXX")"
chmod 0777 "${OPERATOR_DATA}"
OPERATOR_AGENT_ID=""
SUBSIDIARY_SESSION_ID=""

cleanup() {
  if [[ -n "${SUBSIDIARY_SESSION_ID}" ]]; then
    docker rm -f "${SUBSIDIARY_SESSION_ID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${OPERATOR_AGENT_ID}" ]] && curl -fsS "http://127.0.0.1:${DASHBOARD_PORT}/api/status" >/dev/null 2>&1; then
    curl -sS -X POST \
      -H 'content-type: application/json' \
      --data "$(printf '{"agent_id":"%s","force":true}' "${OPERATOR_AGENT_ID}")" \
      "http://127.0.0.1:${DASHBOARD_PORT}/api/orchestrator/stop" >/dev/null 2>&1 || true
  fi
  docker rm -f "${OPERATOR_NAME}" >/dev/null 2>&1 || true
  docker network rm "${NETWORK_NAME}" >/dev/null 2>&1 || true
  if [[ -d "${OPERATOR_DATA}" ]]; then
    docker run --rm -u 0:0 -v "${OPERATOR_DATA}:/data" --entrypoint sh "${IMAGE_TAG}" \
      -lc 'chmod -R a+rwx /data >/dev/null 2>&1 || true' >/dev/null 2>&1 || true
  fi
  rm -rf "${OPERATOR_DATA}"
}
trap cleanup EXIT

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

require_cmd docker
require_cmd curl
require_cmd python3

json_get() {
  local json="$1"
  local path="$2"
  JSON_INPUT="$json" python3 - "$path" <<'PY'
import json
import os
import sys

path = sys.argv[1].split(".")
value = json.loads(os.environ["JSON_INPUT"])
for part in path:
    if part == "":
        continue
    if isinstance(value, list):
        value = value[int(part)]
    else:
        value = value[part]
if isinstance(value, (dict, list)):
    print(json.dumps(value))
elif value is None:
    print("null")
else:
    print(value)
PY
}

request_json() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  local expected="${4:-200}"
  local tmp
  tmp="$(mktemp)"
  local code
  if [[ -n "$body" ]]; then
    code="$(curl -sS -o "$tmp" -w '%{http_code}' \
      -X "$method" \
      -H 'content-type: application/json' \
      --data "$body" \
      "http://127.0.0.1:${DASHBOARD_PORT}/api${path}")"
  else
    code="$(curl -sS -o "$tmp" -w '%{http_code}' \
      -X "$method" \
      "http://127.0.0.1:${DASHBOARD_PORT}/api${path}")"
  fi
  local payload
  payload="$(cat "$tmp")"
  rm -f "$tmp"
  if [[ "$code" != "$expected" ]]; then
    echo "request ${method} ${path} failed: expected ${expected}, got ${code}" >&2
    echo "$payload" >&2
    exit 1
  fi
  printf '%s' "$payload"
}

wait_for_dashboard() {
  local deadline=$((SECONDS + 180))
  while (( SECONDS < deadline )); do
    if curl -fsS "http://127.0.0.1:${DASHBOARD_PORT}/api/status" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "dashboard did not become ready on port ${DASHBOARD_PORT}" >&2
  docker logs "${OPERATOR_NAME}" >&2 || true
  exit 1
}

wait_for_container_session() {
  local session_id="$1"
  local deadline=$((SECONDS + 120))
  while (( SECONDS < deadline )); do
    local payload
    payload="$(request_json GET /containers "" 200)"
    if python3 - "$session_id" <<'PY' <<<"$payload"
import json
import sys

session_id = sys.argv[1]
data = json.load(sys.stdin)
for item in data.get("sessions", []):
    if item.get("session_id") == session_id and item.get("lock_state") is not None:
        sys.exit(0)
sys.exit(1)
PY
    then
      return 0
    fi
    sleep 1
  done
  echo "container session ${session_id} never became mesh-visible" >&2
  request_json GET /containers "" 200 >&2 || true
  exit 1
}

mcp_invoke() {
  local tool="$1"
  local params="$2"
  request_json POST /mcp/invoke "$(printf '{"tool":"%s","params":%s}' "$tool" "$params")" 200
}

if [[ "$BUILD_IMAGE" == "1" ]]; then
  docker build -t "${IMAGE_TAG}" "${ROOT_DIR}"
fi

docker network create "${NETWORK_NAME}" >/dev/null

DOCKER_GID="$(stat -c '%g' /var/run/docker.sock)"

docker run -d \
  --name "${OPERATOR_NAME}" \
  --network "${NETWORK_NAME}" \
  --hostname "${OPERATOR_NAME}" \
  --read-only \
  --tmpfs /tmp:size=100m \
  --tmpfs /run:size=10m \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --group-add "${DOCKER_GID}" \
  -v "${OPERATOR_DATA}:/data" \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -e AGENTHALO_REQUIRE_DASHBOARD_AUTH=0 \
  -e AGENTHALO_CONTAINER_IMAGE="${IMAGE_TAG}" \
  -e NYM_FAIL_OPEN=true \
  -e NYM_MAX_PROVIDER_ATTEMPTS=0 \
  -p "127.0.0.1:${DASHBOARD_PORT}:3100" \
  "${IMAGE_TAG}" >/dev/null

wait_for_dashboard

echo "[1/6] unified container lifecycle: EMPTY"
lock_payload="$(request_json GET /container/lock-status "" 200)"
[[ "$(json_get "$lock_payload" state)" == "empty" ]]

echo "[2/6] initialize self container agent"
init_payload="$(request_json POST /container/initialize '{"hookup":{"kind":"cli","cli_name":"shell"},"reuse_policy":"single_use"}' 200)"
[[ "$(json_get "$init_payload" state)" == "locked" ]]

echo "[3/6] reject second initialization"
request_json POST /container/initialize '{"hookup":{"kind":"cli","cli_name":"shell"},"reuse_policy":"single_use"}' 409 >/dev/null

echo "[4/6] deinitialize back to EMPTY"
deinit_payload="$(request_json POST /container/deinitialize '{}' 200)"
[[ "$(json_get "$deinit_payload" state)" == "empty" ]]

echo "[5/6] unified local model pull"
if [[ "$RUN_MODEL_PULL" == "1" ]] || [[ "$RUN_MODEL_PULL" == "auto" && "$(docker exec "${OPERATOR_NAME}" sh -lc 'command -v hf >/dev/null 2>&1 || command -v huggingface-cli >/dev/null 2>&1; echo $?')" == "0" ]]; then
  pull_payload="$(request_json POST /models/pull "$(printf '{"model":"%s","source":"vllm"}' "${MODEL_ID}")" 200)"
  [[ "$(json_get "$pull_payload" ok)" == "True" || "$(json_get "$pull_payload" ok)" == "true" ]]
else
  echo "  skipped: huggingface CLI not installed in container image"
fi

echo "[6/6] operator -> subsidiary mesh cycle"
launch_payload="$(request_json POST /orchestrator/launch '{"agent":"shell","agent_name":"operator-shell","timeout_secs":30,"trace":false,"capabilities":["operator"],"dispatch_mode":"container","container_hookup":{"kind":"cli","cli_name":"shell"},"admission_mode":"force"}' 200)"
operator_agent_id="$(json_get "$launch_payload" agent_id)"
OPERATOR_AGENT_ID="${operator_agent_id}"

sub_provision="$(mcp_invoke "nucleusdb_subsidiary_provision" "$(printf '{"operator_agent_id":"%s","image":"%s","agent_id":"subsidiary-shell","command":["agenthalo-mcp-server"],"runtime_runsc":false,"env":{},"mesh":{"enabled":true},"admission_mode":"force"}' "${operator_agent_id}" "${IMAGE_TAG}")")"
sub_session_id="$(json_get "$sub_provision" result.structured_content.session_id)"
SUBSIDIARY_SESSION_ID="${sub_session_id}"
wait_for_container_session "${sub_session_id}"

sub_init="$(mcp_invoke "nucleusdb_subsidiary_initialize" "$(printf '{"operator_agent_id":"%s","session_id":"%s","hookup":{"kind":"cli","cli_name":"shell"},"reuse_policy":"single_use"}' "${operator_agent_id}" "${sub_session_id}")")"
[[ "$(json_get "$sub_init" result.structured_content.state)" == "locked" ]]

sub_task="$(mcp_invoke "nucleusdb_subsidiary_send_task" "$(printf '{"operator_agent_id":"%s","session_id":"%s","prompt":"printf subsidiary-ok"}' "${operator_agent_id}" "${sub_session_id}")")"
task_id="$(json_get "$sub_task" result.structured_content.task_id)"

sub_result="$(mcp_invoke "nucleusdb_subsidiary_get_result" "$(printf '{"operator_agent_id":"%s","task_id":"%s"}' "${operator_agent_id}" "${task_id}")")"
[[ "$(json_get "$sub_result" result.structured_content.status)" == "complete" ]]
python3 - <<'PY' "$(json_get "$sub_result" result.structured_content.result)"
import sys
assert "subsidiary-ok" in sys.argv[1], sys.argv[1]
PY

sub_deinit="$(mcp_invoke "nucleusdb_subsidiary_deinitialize" "$(printf '{"operator_agent_id":"%s","session_id":"%s"}' "${operator_agent_id}" "${sub_session_id}")")"
[[ "$(json_get "$sub_deinit" result.structured_content.state)" == "empty" ]]

sub_destroy="$(mcp_invoke "nucleusdb_subsidiary_destroy" "$(printf '{"operator_agent_id":"%s","session_id":"%s"}' "${operator_agent_id}" "${sub_session_id}")")"
[[ "$(json_get "$sub_destroy" result.structured_content.destroyed)" == "True" || "$(json_get "$sub_destroy" result.structured_content.destroyed)" == "true" ]]

echo "PASS: container architecture integration"
