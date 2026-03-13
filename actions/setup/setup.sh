#!/usr/bin/env bash

set -euo pipefail

GH_AW_REF="${GH_AW_SETUP_REF:-v0.57.2}"
ARCHIVE_URL="https://github.com/github/gh-aw/archive/refs/tags/${GH_AW_REF}.tar.gz"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

echo "Bootstrapping gh-aw setup assets from ${ARCHIVE_URL}"

curl -fsSL "${ARCHIVE_URL}" -o "${TMP_DIR}/gh-aw.tar.gz"
tar -xzf "${TMP_DIR}/gh-aw.tar.gz" -C "${TMP_DIR}"

SETUP_DIR="$(find "${TMP_DIR}" -path '*/actions/setup' -type d | head -n 1)"
if [[ -z "${SETUP_DIR}" ]]; then
  echo "::error::Unable to find actions/setup in downloaded gh-aw archive"
  exit 1
fi

echo "Using upstream setup action from ${SETUP_DIR}"
bash "${SETUP_DIR}/setup.sh"
