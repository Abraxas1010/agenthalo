#!/usr/bin/env bash
# Agent H.A.L.O. installer — clone and build from source.
# Usage: curl -fsSL https://raw.githubusercontent.com/Abraxas1010/agenthalo/master/install.sh | bash
#
# Requires: git, Rust toolchain (cargo)
# For private repo access: gh CLI (authenticated) or SSH keys configured
set -euo pipefail

REPO="Abraxas1010/agenthalo"
BINARY="agenthalo"
INSTALL_DIR="${AGENTHALO_INSTALL_DIR:-$HOME/.local/bin}"
BUILD_DIR="${AGENTHALO_BUILD_DIR:-$(mktemp -d)}"
KEEP_SOURCE="${AGENTHALO_KEEP_SOURCE:-false}"

# ── Check prerequisites ─────────────────────────────────────────────────────
check_prereqs() {
  local missing=()

  if ! command -v cargo &>/dev/null; then
    missing+=("cargo (Rust toolchain — install from https://rustup.rs)")
  fi

  if ! command -v git &>/dev/null; then
    missing+=("git")
  fi

  if [ ${#missing[@]} -gt 0 ]; then
    echo "Missing prerequisites:" >&2
    for dep in "${missing[@]}"; do
      echo "  - $dep" >&2
    done
    exit 1
  fi
}

# ── Clone repository ─────────────────────────────────────────────────────────
clone_repo() {
  echo "Cloning Agent H.A.L.O...."

  # Try gh CLI first (handles private repos with auth)
  if command -v gh &>/dev/null; then
    if gh auth status &>/dev/null 2>&1; then
      echo "  Using gh CLI (authenticated)"
      gh repo clone "$REPO" "$BUILD_DIR/agenthalo" -- --depth 1 2>/dev/null && return 0
    fi
  fi

  # Try SSH
  if git ls-remote "git@github.com:${REPO}.git" HEAD &>/dev/null 2>&1; then
    echo "  Using SSH"
    git clone --depth 1 "git@github.com:${REPO}.git" "$BUILD_DIR/agenthalo" && return 0
  fi

  # Try HTTPS (works for public repos or with credential helper)
  if git ls-remote "https://github.com/${REPO}.git" HEAD &>/dev/null 2>&1; then
    echo "  Using HTTPS"
    git clone --depth 1 "https://github.com/${REPO}.git" "$BUILD_DIR/agenthalo" && return 0
  fi

  echo ""
  echo "Clone failed. This is a private repository — you need one of:"
  echo "  1. gh CLI authenticated:  gh auth login"
  echo "  2. SSH key configured:    ssh-keygen -t ed25519 && gh ssh-key add ~/.ssh/id_ed25519.pub"
  echo "  3. HTTPS credential:      git config --global credential.helper store"
  echo ""
  exit 1
}

# ── Build and install ────────────────────────────────────────────────────────
build_and_install() {
  echo "Building Agent H.A.L.O. (release mode)..."
  echo "  This may take a few minutes on first build."
  echo ""

  cd "$BUILD_DIR/agenthalo"
  cargo build --release --bin "$BINARY" 2>&1

  mkdir -p "$INSTALL_DIR"
  cp "target/release/${BINARY}" "$INSTALL_DIR/${BINARY}"
  chmod +x "$INSTALL_DIR/${BINARY}"

  echo ""
  echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
}

# ── Cleanup ──────────────────────────────────────────────────────────────────
cleanup() {
  if [ "$KEEP_SOURCE" = "true" ]; then
    echo "  Source kept at: $BUILD_DIR/agenthalo"
  else
    rm -rf "$BUILD_DIR"
  fi
}

# ── Check PATH ───────────────────────────────────────────────────────────────
check_path() {
  if ! echo ":$PATH:" | grep -q ":${INSTALL_DIR}:"; then
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not in your PATH."
    echo "Add it with:"
    echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
  fi
}

# ── Main ─────────────────────────────────────────────────────────────────────
check_prereqs
clone_repo
trap cleanup EXIT
build_and_install
check_path

echo ""
echo "Get started:"
echo "  agenthalo setup       # Interactive wizard"
echo "  agenthalo dashboard   # Web dashboard"
echo "  agenthalo doctor      # Check everything"
echo ""
