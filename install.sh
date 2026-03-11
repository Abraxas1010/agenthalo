#!/usr/bin/env bash
set -euo pipefail

REPO="Abraxas1010/nucleusdb"
BINARY="nucleusdb"
INSTALL_DIR="${NUCLEUSDB_INSTALL_DIR:-$HOME/.local/bin}"
BUILD_DIR="${NUCLEUSDB_BUILD_DIR:-$(mktemp -d)}"
KEEP_SOURCE="${NUCLEUSDB_KEEP_SOURCE:-false}"

check_prereqs() {
  local missing=()
  command -v cargo >/dev/null 2>&1 || missing+=("cargo (Rust toolchain — install from https://rustup.rs)")
  command -v git >/dev/null 2>&1 || missing+=("git")
  if [ ${#missing[@]} -gt 0 ]; then
    echo "Missing prerequisites:" >&2
    for dep in "${missing[@]}"; do
      echo "  - $dep" >&2
    done
    exit 1
  fi
}

clone_repo() {
  echo "Cloning NucleusDB..."
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    gh repo clone "$REPO" "$BUILD_DIR/nucleusdb" -- --depth 1 2>/dev/null && return 0
  fi
  if git ls-remote "git@github.com:${REPO}.git" HEAD >/dev/null 2>&1; then
    git clone --depth 1 "git@github.com:${REPO}.git" "$BUILD_DIR/nucleusdb" && return 0
  fi
  git clone --depth 1 "https://github.com/${REPO}.git" "$BUILD_DIR/nucleusdb"
}

build_and_install() {
  cd "$BUILD_DIR/nucleusdb"
  cargo build --release \
    --bin nucleusdb \
    --bin nucleusdb-server \
    --bin nucleusdb-mcp \
    --bin nucleusdb-tui \
    --bin nucleusdb-discord
  mkdir -p "$INSTALL_DIR"
  install -m 0755 target/release/nucleusdb "$INSTALL_DIR/nucleusdb"
  install -m 0755 target/release/nucleusdb-server "$INSTALL_DIR/nucleusdb-server"
  install -m 0755 target/release/nucleusdb-mcp "$INSTALL_DIR/nucleusdb-mcp"
  install -m 0755 target/release/nucleusdb-tui "$INSTALL_DIR/nucleusdb-tui"
  install -m 0755 target/release/nucleusdb-discord "$INSTALL_DIR/nucleusdb-discord"
}

cleanup() {
  if [ "$KEEP_SOURCE" = "true" ]; then
    echo "Source kept at: $BUILD_DIR/nucleusdb"
  else
    rm -rf "$BUILD_DIR"
  fi
}

check_path() {
  if ! echo ":$PATH:" | grep -q ":${INSTALL_DIR}:"; then
    echo "Add ${INSTALL_DIR} to PATH:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
  fi
}

check_prereqs
clone_repo
trap cleanup EXIT
build_and_install
check_path

echo
echo "Installed nucleusdb binaries to ${INSTALL_DIR}"
echo "Quick start:"
echo "  nucleusdb create --db ./records.ndb --backend merkle"
echo "  printf 'SET MODE APPEND_ONLY;\\n' | nucleusdb sql --db ./records.ndb"
echo "  nucleusdb-discord"
