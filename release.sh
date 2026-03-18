#!/usr/bin/env bash
# release.sh — bump version, commit, tag, and push to trigger the release workflow
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--force] <new-version>

  --force       Skip version increment validation (use for re-releases)
  new-version   Semantic version without the 'v' prefix (e.g. 1.2.0)

The script:
  1. Validates the new version is a valid semver increment of the
      current version in Cargo.toml (unless --force is used).
  2. Updates Cargo.toml (and Cargo.lock via 'cargo update -p unfk').
  3. Updates the version badge / mention in README.md if present.
  4. Commits and pushes the version bump to master.
  5. Creates and pushes a 'v<new-version>' tag, which triggers the
     GitHub Actions release workflow.
EOF
    exit 1
}

# Parse a semver string into three integers.
parse_semver() {
    local v="$1"
    # Strip a leading 'v' if present.
    v="${v#v}"
    if [[ ! "$v" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
        die "Not a valid semver: '$v'"
    fi
    echo "${BASH_REMATCH[1]} ${BASH_REMATCH[2]} ${BASH_REMATCH[3]}"
}

# Returns 0 (true) if $2 is a valid single-level increment of $1.
# Allowed increments:
#   major: (X+1).0.0
#   minor: X.(Y+1).0
#   patch: X.Y.(Z+1)
is_valid_increment() {
    local from="$1" to="$2"
    read -r from_maj from_min from_pat <<< "$(parse_semver "$from")"
    read -r to_maj   to_min   to_pat   <<< "$(parse_semver "$to")"

    # major bump
    if (( to_maj == from_maj + 1 && to_min == 0 && to_pat == 0 )); then
        return 0
    fi
    # minor bump (same major)
    if (( to_maj == from_maj && to_min == from_min + 1 && to_pat == 0 )); then
        return 0
    fi
    # patch bump (same major + minor)
    if (( to_maj == from_maj && to_min == from_min && to_pat == from_pat + 1 )); then
        return 0
    fi

    return 1
}

[[ $# -ge 1 ]] || usage

FORCE=false
if [[ "$1" == "--force" ]]; then
    FORCE=true
    shift
fi

[[ $# -eq 1 ]] || usage
NEW_VERSION="${1#v}"   # strip accidental leading 'v'

# Validate it is at least a well-formed semver.
parse_semver "$NEW_VERSION" > /dev/null

# Determine the current version
REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
CARGO_TOML="$REPO_DIR/Cargo.toml"
README="$REPO_DIR/README.md"

[[ -f "$CARGO_TOML" ]] || die "Cargo.toml not found at $CARGO_TOML"

CURRENT_VERSION=$(grep -m1 '^version = ' "$CARGO_TOML" | grep -oP '"\K[^"]+(?=")')
[[ -n "$CURRENT_VERSION" ]] \
    || die "Could not determine the current version from Cargo.toml."

echo "  Current   : $CURRENT_VERSION"
echo "  New       : $NEW_VERSION"

# Validate the increment (skip if this is a retry of same version, or if --force).
TAG="v${NEW_VERSION}"
TAG_EXISTS_REMOTE=false
git ls-remote --exit-code --tags origin "$TAG" &>/dev/null && TAG_EXISTS_REMOTE=true

if $FORCE; then
    echo "Force mode enabled — skipping version increment validation."
elif [[ "$CURRENT_VERSION" == "$NEW_VERSION" ]] && $TAG_EXISTS_REMOTE; then
    echo "Version matches Cargo.toml and tag exists on remote."
    echo "Treating as a retry - skipping version increment validation."
elif ! is_valid_increment "$CURRENT_VERSION" "$NEW_VERSION"; then
    die "'$NEW_VERSION' is not a valid semver increment of '$CURRENT_VERSION'. " \
        "Allowed: $((${CURRENT_VERSION%%.*} + 1)).0.0, " \
        "$(echo "$CURRENT_VERSION" | cut -d. -f1).$(($(echo "$CURRENT_VERSION" | cut -d. -f2) + 1)).0, " \
        "or $CURRENT_VERSION with the patch incremented by 1. Use --force to bypass this check."
fi

echo "Version increment is valid."

cd "$REPO_DIR"
if [[ -n "$(git status --porcelain)" ]]; then
    die "Working tree is not clean. Commit or stash your changes first."
fi

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$CURRENT_BRANCH" != "master" ]]; then
    die "Not on master (current branch: '$CURRENT_BRANCH'). Checkout master before releasing."
fi

# Guard: tag must not already exist locally, unless it is also on the remote
# (indicating a previous workflow run failed and we need to retry).
TAG_EXISTS_LOCALLY=false
TAG_EXISTS_REMOTE=false
git rev-parse "$TAG" &>/dev/null && TAG_EXISTS_LOCALLY=true
git ls-remote --exit-code --tags origin "$TAG" &>/dev/null && TAG_EXISTS_REMOTE=true

if $TAG_EXISTS_LOCALLY && ! $TAG_EXISTS_REMOTE; then
    echo "Tag '$TAG' exists locally but not on remote. Deleting local tag..."
    git tag -d "$TAG"
    TAG_EXISTS_LOCALLY=false
fi

# Handle retry: tag exists on remote - delete it and continue with normal flow
if $TAG_EXISTS_REMOTE; then
    echo "Tag '$TAG' already exists on remote — deleting to allow re-release..."

    # Delete local tag if it exists
    if $TAG_EXISTS_LOCALLY; then
        git tag -d "$TAG"
    fi
    
    # Delete remote tag
    git push origin ":refs/tags/$TAG"
    echo "Remote tag deleted. Proceeding with release..."
fi

# Update Cargo.toml
CURRENT_CARGO_VERSION=$(grep -m1 '^version = ' "$CARGO_TOML" | grep -oP '"\K[^"]+(?=")')

if [[ "$CURRENT_CARGO_VERSION" == "$NEW_VERSION" ]]; then
    echo "Cargo.toml is already at $NEW_VERSION — skipping version bump commit."
else
    echo "Updating Cargo.toml..."
    # Replace the version line in the [package] section only (first occurrence).
    sed -i "0,/^version = \"[^\"]*\"/{s/^version = \"[^\"]*\"/version = \"${NEW_VERSION}\"/}" "$CARGO_TOML"

    # Verify the replacement.
    UPDATED=$(grep -m1 '^version = ' "$CARGO_TOML" | grep -oP '"\K[^"]+(?=")')
    [[ "$UPDATED" == "$NEW_VERSION" ]] \
        || die "Failed to update version in Cargo.toml (got '$UPDATED')."

    # Update Cargo.lock
    echo "Updating Cargo.lock..."
    cargo update -p unfk --precise "$NEW_VERSION" 2>/dev/null \
        || cargo generate-lockfile

    # Update README.md release badge branch reference
    if [[ -f "$README" ]]; then
        echo "Updating release badge branch in README.md..."
        sed -i "s|release\.yml/badge\.svg?branch=v[^)]*|release.yml/badge.svg?branch=v${NEW_VERSION}|g" "$README"
        git add "$README"
    fi

    # Commit and push
    echo "Committing version bump..."
    git add Cargo.toml Cargo.lock
    git commit -m "chore: bump version to ${NEW_VERSION}"

    echo "Pushing commits to origin/master..."
    git push origin master
fi

# Tag and push tag (triggers release workflow)
echo "Creating tag $TAG..."
git tag "$TAG"

echo "Pushing tag $TAG..."
git push origin "$TAG"

echo ""
echo "Done. Tag '$TAG' pushed — the GitHub Actions release workflow should now be running."
echo "https://github.com/keathmilligan/unfk/actions"
