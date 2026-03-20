#!/usr/bin/env bash
# check_diagram_freshness.sh — Pre-push gate for system architecture diagram
#
# Ensures the system architecture diagram has been reviewed within the last
# MAX_STALE_DAYS (default: 14). If stale, the push is blocked with instructions
# for the agent to review and update the diagram.
#
# Usage:
#   ./scripts/check_diagram_freshness.sh          # exit 0 = fresh, exit 1 = stale
#   DIAGRAM_MAX_STALE_DAYS=7 ./scripts/check_diagram_freshness.sh
#
# Called from .git/hooks/pre-push (or equivalent hook runner).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIAGRAM="${REPO_ROOT}/dashboard/agenthalo-system-diagram.html"
MAX_STALE_DAYS="${DIAGRAM_MAX_STALE_DAYS:-14}"

if [ ! -f "$DIAGRAM" ]; then
  echo "WARN: System architecture diagram not found at $DIAGRAM — skipping freshness check."
  exit 0
fi

# Extract DIAGRAM_REVIEW_DATE from the HTML comment header
REVIEW_DATE=$(grep -oP 'DIAGRAM_REVIEW_DATE:\s*\K[0-9]{4}-[0-9]{2}-[0-9]{2}' "$DIAGRAM" 2>/dev/null || echo "")

if [ -z "$REVIEW_DATE" ]; then
  echo "ERROR: No DIAGRAM_REVIEW_DATE found in $DIAGRAM"
  echo ""
  echo "The system architecture diagram must have a review date."
  echo "Add a comment at the top of the file:"
  echo "  <!-- DIAGRAM_REVIEW_DATE: $(date +%Y-%m-%d) -->"
  echo ""
  echo "Then review all Mermaid diagrams against the current codebase."
  exit 1
fi

# Calculate staleness
TODAY=$(date +%Y-%m-%d)
REVIEW_EPOCH=$(date -d "$REVIEW_DATE" +%s 2>/dev/null || date -j -f "%Y-%m-%d" "$REVIEW_DATE" +%s 2>/dev/null || echo 0)
TODAY_EPOCH=$(date +%s)

if [ "$REVIEW_EPOCH" -eq 0 ]; then
  echo "ERROR: Could not parse DIAGRAM_REVIEW_DATE '$REVIEW_DATE'"
  exit 1
fi

DAYS_STALE=$(( (TODAY_EPOCH - REVIEW_EPOCH) / 86400 ))

if [ "$DAYS_STALE" -gt "$MAX_STALE_DAYS" ]; then
  echo "=========================================="
  echo "  DIAGRAM FRESHNESS CHECK FAILED"
  echo "=========================================="
  echo ""
  echo "  Last reviewed:  $REVIEW_DATE ($DAYS_STALE days ago)"
  echo "  Max staleness:  $MAX_STALE_DAYS days"
  echo "  Diagram:        dashboard/agenthalo-system-diagram.html"
  echo ""
  echo "  Before pushing, you MUST:"
  echo "  1. Review each Mermaid diagram section against the current codebase"
  echo "  2. Update any diagrams that no longer match (new files, renamed modules, etc.)"
  echo "  3. Update DIAGRAM_REVIEW_DATE to today's date: $TODAY"
  echo "  4. Update DIAGRAM_REVIEWER to your identity"
  echo "  5. Update the 'Reviewed:' badge in the header element"
  echo ""
  echo "  Key areas to check:"
  echo "  - Section 2 (Binary Targets): compare against Cargo.toml [[bin]] entries"
  echo "  - Section 3 (Module Architecture): compare against src/ directory structure"
  echo "  - Section 10 (Dashboard Frontend): compare against dashboard/*.js files"
  echo "  - Section 11 (MCP Tool Surface): compare against MCP tool registrations"
  echo "  - Section 15 (Complete File Map): compare against actual file tree"
  echo ""
  exit 1
fi

echo "Diagram freshness OK: reviewed $REVIEW_DATE ($DAYS_STALE days ago, limit $MAX_STALE_DAYS)"
exit 0
