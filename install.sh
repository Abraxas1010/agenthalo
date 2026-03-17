#!/usr/bin/env bash
set -euo pipefail

REPO="Abraxas1010/agenthalo"
PROJECT_DIR_NAME="agenthalo"
INSTALL_DIR="${AGENTHALO_INSTALL_DIR:-${NUCLEUSDB_INSTALL_DIR:-$HOME/.local/bin}}"
BUILD_DIR="${AGENTHALO_BUILD_DIR:-${NUCLEUSDB_BUILD_DIR:-$(mktemp -d)}}"
KEEP_SOURCE="${AGENTHALO_KEEP_SOURCE:-${NUCLEUSDB_KEEP_SOURCE:-false}}"
BINARIES=(
  agenthalo
  agenthalo-mcp-server
  nucleusdb
  nucleusdb-server
  nucleusdb-mcp
  nucleusdb-tui
  nucleusdb-discord
)

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
  echo "Cloning AgentHALO..."
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    gh repo clone "$REPO" "$BUILD_DIR/$PROJECT_DIR_NAME" -- --depth 1 2>/dev/null && return 0
  fi
  if git ls-remote "git@github.com:${REPO}.git" HEAD >/dev/null 2>&1; then
    git clone --depth 1 "git@github.com:${REPO}.git" "$BUILD_DIR/$PROJECT_DIR_NAME" && return 0
  fi
  git clone --depth 1 "https://github.com/${REPO}.git" "$BUILD_DIR/$PROJECT_DIR_NAME"
}

build_and_install() {
  local cargo_args=()
  cd "$BUILD_DIR/$PROJECT_DIR_NAME"
  for binary in "${BINARIES[@]}"; do
    cargo_args+=(--bin "$binary")
  done
  cargo build --release "${cargo_args[@]}"
  mkdir -p "$INSTALL_DIR"
  for binary in "${BINARIES[@]}"; do
    install -m 0755 "target/release/$binary" "$INSTALL_DIR/$binary"
  done
}

cleanup() {
  if [ "$KEEP_SOURCE" = "true" ]; then
    echo "Source kept at: $BUILD_DIR/$PROJECT_DIR_NAME"
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
echo "Installed AgentHALO binaries to ${INSTALL_DIR}"
echo "Quick start:"
echo "  agenthalo run claude"
echo "  agenthalo dashboard --port 3100"
echo "  nucleusdb create --db ./records.ndb --backend merkle"
