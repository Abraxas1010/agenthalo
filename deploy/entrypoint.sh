#!/usr/bin/env bash
set -euo pipefail
DB="${NUCLEUSDB_DISCORD_DB_PATH:-/data/discord_records.ndb}"
if [ ! -f "$DB" ]; then
  nucleusdb create --db "$DB" --backend merkle
  printf 'SET MODE APPEND_ONLY;\n' | nucleusdb sql --db "$DB"
fi
nucleusdb-server "${NUCLEUSDB_API_ADDR:-0.0.0.0:8088}" production &
nucleusdb mcp --transport http --host "${NUCLEUSDB_MCP_HOST:-0.0.0.0}" --port "${NUCLEUSDB_MCP_PORT:-3000}" --db "$DB" &
nucleusdb dashboard --port "${NUCLEUSDB_DASHBOARD_PORT:-3100}" --no-open &
exec nucleusdb-discord
