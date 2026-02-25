#!/usr/bin/env bash
# Agent H.A.L.O. installer — single-binary install from GitHub releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/Abraxas1010/agenthalo/master/install.sh | bash
set -euo pipefail

REPO="Abraxas1010/agenthalo"
BINARY="agenthalo"
VERSION="${AGENTHALO_VERSION:-latest}"
INSTALL_DIR="${AGENTHALO_INSTALL_DIR:-$HOME/.local/bin}"

# ── Detect platform ──────────────────────────────────────────────────────────
detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)  os="linux" ;;
    Darwin) os="darwin" ;;
    *)      echo "Unsupported OS: $os" >&2; exit 1 ;;
  esac

  case "$arch" in
    x86_64|amd64)       arch="x86_64" ;;
    aarch64|arm64)      arch="aarch64" ;;
    *)                  echo "Unsupported architecture: $arch" >&2; exit 1 ;;
  esac

  echo "${os}-${arch}"
}

# ── Resolve version ──────────────────────────────────────────────────────────
resolve_version() {
  if [ "$VERSION" = "latest" ]; then
    VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')"
    if [ -z "$VERSION" ]; then
      echo "Could not determine latest version. Set AGENTHALO_VERSION manually." >&2
      exit 1
    fi
  fi
}

# ── Download and install ─────────────────────────────────────────────────────
install() {
  local platform asset_name url tmp

  platform="$(detect_platform)"
  asset_name="${BINARY}-${VERSION}-${platform}"
  url="https://github.com/${REPO}/releases/download/${VERSION}/${asset_name}.tar.gz"

  echo "Installing Agent H.A.L.O. ${VERSION} (${platform})..."
  echo "  From: ${url}"
  echo "  To:   ${INSTALL_DIR}/${BINARY}"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  if ! curl -fsSL "$url" -o "$tmp/archive.tar.gz"; then
    echo ""
    echo "Download failed. No pre-built release found for ${VERSION} / ${platform}."
    echo ""
    echo "To build from source instead:"
    echo "  git clone https://github.com/${REPO}.git && cd agenthalo"
    echo "  cargo install --path . --bin agenthalo"
    echo ""
    exit 1
  fi

  tar -xzf "$tmp/archive.tar.gz" -C "$tmp"

  mkdir -p "$INSTALL_DIR"
  mv "$tmp/${BINARY}" "$INSTALL_DIR/${BINARY}"
  chmod +x "$INSTALL_DIR/${BINARY}"

  echo ""
  echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

  # Check PATH
  if ! echo ":$PATH:" | grep -q ":${INSTALL_DIR}:"; then
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not in your PATH."
    echo "Add it with:"
    echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
  fi

  echo ""
  echo "Get started:"
  echo "  agenthalo setup       # Interactive wizard"
  echo "  agenthalo dashboard   # Web dashboard"
  echo "  agenthalo doctor      # Check everything"
  echo ""
}

resolve_version
install
