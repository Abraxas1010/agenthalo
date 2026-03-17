#!/usr/bin/env bash
set -euo pipefail
BINARIES=(
  agenthalo
  agenthalo-mcp-server
  nucleusdb
  nucleusdb-mcp
  nucleusdb-server
  nucleusdb-tui
  nucleusdb-discord
)
cargo_args=()
for binary in "${BINARIES[@]}"; do
  cargo_args+=(--bin "$binary")
done
cargo build --release "${cargo_args[@]}"
for binary in "${BINARIES[@]}"; do
  sudo install -m 0755 "target/release/$binary" /usr/local/bin/
done
sudo useradd --system --home-dir /var/lib/agenthalo --create-home --shell /usr/sbin/nologin agenthalo || true
sudo mkdir -p /etc/agenthalo
sudo cp deploy/discord.env.example /etc/agenthalo/discord.env
sudo chmod 600 /etc/agenthalo/discord.env
sudo cp deploy/agenthalo-*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable agenthalo-discord agenthalo-dashboard

cat <<'EOF'

Installed and enabled:
  - agenthalo-dashboard.service
  - agenthalo-discord.service

Optional services:
  - agenthalo-mcp.service
  - agenthalo-p2p-bridge.service

Before enabling agenthalo-mcp.service, set AGENTHALO_MCP_SECRET in:
  /etc/agenthalo/discord.env

Example:
  sudo sh -c 'printf "\nAGENTHALO_MCP_SECRET=%s\n" "$(openssl rand -hex 32)" >> /etc/agenthalo/discord.env'
  sudo systemctl enable --now agenthalo-mcp

Enable the P2P bridge only after its environment is configured:
  sudo systemctl enable --now agenthalo-p2p-bridge
EOF
