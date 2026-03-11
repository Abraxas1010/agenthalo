#!/usr/bin/env bash
set -euo pipefail
cargo build --release --bin nucleusdb --bin nucleusdb-mcp --bin nucleusdb-server --bin nucleusdb-tui --bin nucleusdb-discord
sudo install -m 0755 target/release/nucleusdb /usr/local/bin/
sudo install -m 0755 target/release/nucleusdb-mcp /usr/local/bin/
sudo install -m 0755 target/release/nucleusdb-server /usr/local/bin/
sudo install -m 0755 target/release/nucleusdb-tui /usr/local/bin/
sudo install -m 0755 target/release/nucleusdb-discord /usr/local/bin/
sudo useradd --system --home-dir /var/lib/nucleusdb --create-home --shell /usr/sbin/nologin nucleusdb || true
sudo mkdir -p /etc/nucleusdb
sudo cp deploy/discord.env.example /etc/nucleusdb/discord.env
sudo chmod 600 /etc/nucleusdb/discord.env
sudo cp deploy/*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable nucleusdb-discord nucleusdb-mcp nucleusdb-dashboard
