#!/usr/bin/env bash
set -euo pipefail

REPO="salamaashoush/sweeprs"
BINARY_NAME="sweeprs"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect architecture
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  TARGET="x86_64-apple-darwin" ;;
    arm64)   TARGET="aarch64-apple-darwin" ;;
    aarch64) TARGET="aarch64-apple-darwin" ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# Detect OS
OS=$(uname -s)
if [ "$OS" != "Darwin" ]; then
    echo "sweeprs only supports macOS. Detected: $OS"
    exit 1
fi

# Get latest version
if [ -n "${VERSION:-}" ]; then
    TAG="v${VERSION#v}"
else
    echo "Fetching latest release..."
    TAG=$(curl -sL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$TAG" ]; then
        echo "Failed to fetch latest release tag"
        exit 1
    fi
fi

VERSION="${TAG#v}"
ARCHIVE="${BINARY_NAME}-${TAG}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

echo "Installing ${BINARY_NAME} ${TAG} for ${TARGET}..."

# Download
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading ${URL}..."
if ! curl -fsSL "$URL" -o "${TMPDIR}/${ARCHIVE}"; then
    echo "Failed to download ${URL}"
    echo "Check available releases at: https://github.com/${REPO}/releases"
    exit 1
fi

# Extract
tar -xzf "${TMPDIR}/${ARCHIVE}" -C "$TMPDIR"

# Install
if [ -w "$INSTALL_DIR" ]; then
    mv "${TMPDIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
else
    echo "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mv "${TMPDIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
fi

chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "${BINARY_NAME} ${TAG} installed to ${INSTALL_DIR}/${BINARY_NAME}"
echo ""
echo "Run 'sweeprs --help' to get started."
