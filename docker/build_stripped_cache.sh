#!/usr/bin/env bash
set -euo pipefail

ENGINE="${NUCLEUSDB_CONTAINER_ENGINE:-}"
if [[ -z "${ENGINE}" ]]; then
  if command -v podman >/dev/null 2>&1; then
    ENGINE="podman"
  else
    ENGINE="docker"
  fi
fi

IMAGE_TAG="${IMAGE_TAG:-heytinglean-compile}"
HEYTINGLEAN_REPO="${HEYTINGLEAN_REPO:-}"
STAGING_DIR="docker/.heytinglean-build"

echo "Building stripped HeytingLean cache image"
echo "  engine: ${ENGINE}"
echo "  tag:    ${IMAGE_TAG}"

if [[ -z "${HEYTINGLEAN_REPO}" ]]; then
  if [[ -d "../heyting/.git" ]]; then
    HEYTINGLEAN_REPO="$(cd ../heyting && pwd)"
  elif [[ -d "/home/abraxas/Work/heyting/.git" ]]; then
    HEYTINGLEAN_REPO="/home/abraxas/Work/heyting"
  else
    echo "Set HEYTINGLEAN_REPO to a local heyting checkout" >&2
    exit 1
  fi
fi

echo "  repo:   ${HEYTINGLEAN_REPO}"

rm -rf "${STAGING_DIR}"
mkdir -p "${STAGING_DIR}/HeytingLean"
mkdir -p "${STAGING_DIR}/packages"
rsync -a --delete \
  --delete-excluded \
  --exclude '.lake' \
  --exclude 'build' \
  --exclude 'dist' \
  --exclude 'node_modules' \
  --exclude '.git' \
  "${HEYTINGLEAN_REPO}/HeytingLean/" "${STAGING_DIR}/HeytingLean/"
cp "${HEYTINGLEAN_REPO}/HeytingLean.lean" "${STAGING_DIR}/HeytingLean.lean"
cp "${HEYTINGLEAN_REPO}/lakefile.lean" "${STAGING_DIR}/lakefile.lean"
cp "${HEYTINGLEAN_REPO}/lean-toolchain" "${STAGING_DIR}/lean-toolchain"
if [[ -f "${HEYTINGLEAN_REPO}/lake-manifest.json" ]]; then
  cp "${HEYTINGLEAN_REPO}/lake-manifest.json" "${STAGING_DIR}/lake-manifest.json"
fi
for pkg in PhysLean Foundation; do
  if [[ -d "${HEYTINGLEAN_REPO}/.lake/packages/${pkg}" ]]; then
    rsync -a --delete "${HEYTINGLEAN_REPO}/.lake/packages/${pkg}/" "${STAGING_DIR}/packages/${pkg}/"
  fi
done

"${ENGINE}" build \
  -f docker/Dockerfile.heytinglean \
  -t "${IMAGE_TAG}" \
  .

echo "Image size bytes:"
"${ENGINE}" image inspect "${IMAGE_TAG}" --format '{{.Size}}'
