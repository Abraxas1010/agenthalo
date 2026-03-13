#!/usr/bin/env bash
set -euo pipefail
SRC="/tmp/heyting_living_agent_impl_20260312T201544Z/artifacts/living_agent/verified_grid"
DEST="/tmp/the-living-agent/knowledge/grid"
DEST_INDEX="/tmp/the-living-agent/knowledge/grid_index.md"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
if [ -d "$DEST" ]; then
  mv "$DEST" "${DEST}.bak.$STAMP"
fi
mkdir -p "$(dirname "$DEST")"
mkdir -p "$DEST"
cp -a "$SRC/grid/." "$DEST/"
cp "$SRC/grid_index.md" "$DEST_INDEX"
echo "Installed verified grid into /tmp/the-living-agent"
