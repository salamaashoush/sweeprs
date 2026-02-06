#!/usr/bin/env bash
set -euo pipefail

VERSION="$1"
VERSION="${VERSION#v}"  # Remove v prefix
DIST_DIR="dist"

# Check prerequisites
if ! command -v gh &> /dev/null; then
    echo "gh (GitHub CLI) not found. Install it from: https://cli.github.com/"
    exit 1
fi

# Check if artifacts exist
if [ ! -d "$DIST_DIR" ] || [ -z "$(ls -A $DIST_DIR 2>/dev/null)" ]; then
    echo "No artifacts found in ${DIST_DIR}/"
    echo "Run: just gh-build-release v${VERSION}"
    exit 1
fi

echo "Creating GitHub release v${VERSION}..."

# Generate release notes
RELEASE_NOTES=""

# Try to extract changelog for this specific version from CHANGELOG.md
if [ -f "CHANGELOG.md" ]; then
    echo "Extracting changelog for v${VERSION}..."
    # Extract only the section for this version (from ## [VERSION] to the next ## [)
    START_LINE=$(grep -n "^## \[${VERSION}\]" CHANGELOG.md | cut -d: -f1 | head -1 || true)
    if [ -n "$START_LINE" ]; then
        NEXT_LINE=$(tail -n +$((START_LINE + 1)) CHANGELOG.md | grep -n "^## \[" | head -1 | cut -d: -f1 || true)
        if [ -n "$NEXT_LINE" ]; then
            # Extract from START_LINE to NEXT_LINE (excluding header and next version)
            CHANGELOG=$(sed -n "$((START_LINE + 1)),$((START_LINE + NEXT_LINE - 1))p" CHANGELOG.md)
        else
            # No next version, extract to end of file
            CHANGELOG=$(tail -n +$((START_LINE + 1)) CHANGELOG.md)
        fi
    else
        CHANGELOG=""
    fi

    if [ -z "$CHANGELOG" ]; then
        echo "Version ${VERSION} not found in CHANGELOG.md"
        echo "   Make sure to run: just gh-changelog ${VERSION}"
    fi
else
    echo "CHANGELOG.md not found"
    echo "   Run: just gh-changelog ${VERSION}"
    CHANGELOG=""
fi

# If Claude CLI is available, enhance the release notes
CLAUDE_BIN=""
if [ -x "$HOME/.claude/local/claude" ]; then
    CLAUDE_BIN="$HOME/.claude/local/claude"
elif command -v claude &> /dev/null; then
    CLAUDE_BIN="claude"
fi

if [ -n "$CLAUDE_BIN" ] && [ -n "$CHANGELOG" ]; then
    echo "Enhancing release notes with Claude AI..."

    PROMPT=$(printf '%s\n' \
        "You are a technical writer creating GitHub release notes" \
        "" \
        "Given the following git changelog, create professional, well-formatted release notes in Markdown format" \
        "" \
        "Requirements:" \
        "- Use clear, concise language" \
        "- Group changes by type (Features, Bug Fixes, Documentation, etc)" \
        "- Highlight breaking changes if any" \
        "- Keep it developer-friendly but professional" \
        "- Include a brief summary at the top" \
        "- Format code/commands in backticks" \
        "- Keep lists clean and scannable" \
        "- DO NOT include an installation section" \
        "- Instead, add a note at the end: 'For installation instructions, see the README'" \
        "" \
        "Changelog:" \
        "$CHANGELOG")

    if RELEASE_NOTES=$(echo "$PROMPT" | "$CLAUDE_BIN" --print 2>/dev/null) && [ -n "$RELEASE_NOTES" ]; then
        echo "Enhanced release notes generated with AI"
    else
        echo "Claude CLI failed or returned empty result, using raw changelog"
        RELEASE_NOTES="$CHANGELOG"
    fi
else
    RELEASE_NOTES="$CHANGELOG"
    if [ -z "$CLAUDE_BIN" ]; then
        echo "Claude CLI not found - using raw changelog"
    fi
fi

# Fallback if no changelog available
if [ -z "$RELEASE_NOTES" ]; then
    REPO_FULL=$(gh repo view --json nameWithOwner -q .nameWithOwner)
    RELEASE_NOTES=$(printf "Release v%s\n\nSee the full list of changes in the [commit history](https://github.com/%s/compare/v%s)" "${VERSION}" "${REPO_FULL}" "${VERSION}")
fi

# Create release
gh release create "v${VERSION}" \
    --title "v${VERSION}" \
    --notes "${RELEASE_NOTES}" \
    "${DIST_DIR}"/*

REPO_URL=$(gh repo view --json url -q .url)
echo "Release v${VERSION} created successfully!"
echo "View at: ${REPO_URL}/releases/tag/v${VERSION}"
