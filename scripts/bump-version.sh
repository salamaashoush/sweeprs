#!/usr/bin/env bash
set -euo pipefail

TYPE="$1"

# Validate type
if [[ ! "$TYPE" =~ ^(major|minor|patch)$ ]]; then
    echo "Invalid version type: $TYPE"
    echo "Usage: scripts/bump-version.sh [major|minor|patch]"
    exit 1
fi

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

if [ -z "$CURRENT_VERSION" ]; then
    echo "Could not find current version in Cargo.toml"
    exit 1
fi

echo "Current version: v${CURRENT_VERSION}"

# Parse version components
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

# Bump version based on type
case "$TYPE" in
    major)
        MAJOR=$((MAJOR + 1))
        MINOR=0
        PATCH=0
        ;;
    minor)
        MINOR=$((MINOR + 1))
        PATCH=0
        ;;
    patch)
        PATCH=$((PATCH + 1))
        ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
echo "New version: v${NEW_VERSION}"

# Update Cargo.toml
echo "Updating Cargo.toml..."
sed -i.bak "s/^version = \"${CURRENT_VERSION}\"/version = \"${NEW_VERSION}\"/" Cargo.toml
rm Cargo.toml.bak

# Update Cargo.lock
cargo update --workspace

echo "Version bumped to v${NEW_VERSION}"
echo ""
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Run: just release ${TYPE}"
