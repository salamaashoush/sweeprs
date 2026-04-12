#!/usr/bin/env bash
set -euo pipefail

VERSION="$1"
VERSION="${VERSION#v}"  # Remove v prefix if present
DIST_DIR="dist"
BINARY_NAME="sweeprs"

echo "Packaging binaries for GitHub release v${VERSION}..."
rm -rf "${DIST_DIR}"
mkdir -p "${DIST_DIR}"

# Package function
package_binary() {
    local target=$1
    local archive_name="${BINARY_NAME}-v${VERSION}-${target}"

    if [[ "$target" == *"windows"* ]]; then
        local binary_path="target/${target}/release/${BINARY_NAME}.exe"
        local archive="${DIST_DIR}/${archive_name}.zip"
        if [ -f "$binary_path" ]; then
            zip -j -q "${archive}" "$binary_path"
            echo "Created: ${archive}"
        else
            echo "Binary not found: ${binary_path}"
        fi
    else
        local binary_path="target/${target}/release/${BINARY_NAME}"
        local archive="${DIST_DIR}/${archive_name}.tar.gz"
        if [ -f "$binary_path" ]; then
            tar -czf "${archive}" -C "$(dirname "$binary_path")" "$(basename "$binary_path")"
            echo "Created: ${archive}"
        else
            echo "Binary not found: ${binary_path}"
        fi
    fi
}

# Package all targets
for target in x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
    package_binary "$target"
done

echo ""
echo "Artifacts in ${DIST_DIR}:"
ls -lh "${DIST_DIR}"
