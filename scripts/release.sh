#!/usr/bin/env bash
set -euo pipefail

TYPE="$1"  # patch, minor, or major

echo "Starting release process for ${TYPE} version bump..."
echo ""

# Check if we already have a version bump commit and tag (from a previous failed run)
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
LAST_COMMIT_MSG=$(git log -1 --pretty=%B 2>/dev/null || echo "")
CURRENT_TAG=$(git tag -l "v${CURRENT_VERSION}" 2>/dev/null || echo "")

if [[ "$LAST_COMMIT_MSG" =~ ^chore:\ (release|bump\ version\ to)\ v?${CURRENT_VERSION}$ ]] && [ -n "$CURRENT_TAG" ]; then
    echo "Version v${CURRENT_VERSION} already committed and tagged"
    echo "   Last commit: $LAST_COMMIT_MSG"
    echo "   Tag exists: v${CURRENT_TAG}"
    echo ""
    echo "Skipping version bump, changelog, commit, tag, and push steps"
    echo "   (Previous release attempt detected - resuming from build step)"
    echo ""
    NEW_VERSION="${CURRENT_VERSION}"
    SKIP_BUMP_AND_COMMIT=true
else
    SKIP_BUMP_AND_COMMIT=false
fi

if [ "$SKIP_BUMP_AND_COMMIT" = false ]; then
    # Step 1: Bump version in Cargo.toml
    echo "Step 1/7: Bumping version..."
    scripts/bump-version.sh "${TYPE}"

    # Get the new version
    NEW_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    echo "New version: ${NEW_VERSION}"
    echo ""

    # Step 2: Get the previous tag
    echo "Step 2/7: Finding previous tag..."
    PREV_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
    if [ -n "$PREV_TAG" ]; then
        echo "Previous tag: ${PREV_TAG}"
    else
        echo "No previous tag found (first release)"
    fi
    echo ""

    # Step 3: Generate changelog
    echo "Step 3/7: Generating changelog..."
    if [ -n "$PREV_TAG" ]; then
        if [ -f CHANGELOG.md ]; then
            git cliff "${PREV_TAG}..HEAD" --tag "v${NEW_VERSION}" --prepend CHANGELOG.md
        else
            git cliff "${PREV_TAG}..HEAD" --tag "v${NEW_VERSION}" --output CHANGELOG.md
        fi
    else
        git cliff --tag "v${NEW_VERSION}" --output CHANGELOG.md
    fi
    echo "Changelog generated"
    echo ""

    # Step 4: Commit and tag
    echo "Step 4/7: Committing and tagging..."
    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit -m "chore: release v${NEW_VERSION}"
    git tag -a "v${NEW_VERSION}" -m "Release v${NEW_VERSION}"
    echo "Committed and tagged v${NEW_VERSION}"
    echo ""

    # Step 5: Push to remote
    echo "Step 5/7: Pushing to remote..."
    git push && git push --tags
    echo "Pushed to remote"
    echo ""
else
    echo "Steps 1-5/7: Skipped (already completed in previous run)"
    echo ""
fi

# Step 6: Build all binaries
echo "Step 6/7: Building all platform binaries..."
just cross-build-all
echo "Binaries built"
echo ""

# Step 7: Package and create GitHub release
echo "Step 7/7: Creating GitHub release..."
scripts/gh-package.sh "${NEW_VERSION}"
scripts/gh-release.sh "${NEW_VERSION}"
echo "GitHub release created"
echo ""

echo "Release v${NEW_VERSION} completed successfully!"
