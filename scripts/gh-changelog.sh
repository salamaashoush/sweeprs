#!/usr/bin/env bash
set -euo pipefail

VERSION="$1"
VERSION="${VERSION#v}"  # Remove v prefix
echo "Generating changelog for v${VERSION}..."

# If CHANGELOG.md doesn't exist, create it from scratch
if [ ! -f "CHANGELOG.md" ]; then
    git cliff --tag "v${VERSION}" --output CHANGELOG.md
else
    # Get the previous tag to generate changelog for the range
    PREV_TAG=$(git describe --tags --abbrev=0 "v${VERSION}^" 2>/dev/null || echo "")

    if [ -n "$PREV_TAG" ]; then
        # Generate changelog for commits between previous tag and HEAD
        echo "Generating changelog from ${PREV_TAG} to HEAD for v${VERSION}"
        git cliff "${PREV_TAG}..HEAD" --tag "v${VERSION}" --prepend CHANGELOG.md
    else
        # No previous tag, generate from beginning
        echo "No previous tag found, generating full changelog"
        git cliff --tag "v${VERSION}" --unreleased --prepend CHANGELOG.md
    fi
fi

echo "CHANGELOG.md updated"
