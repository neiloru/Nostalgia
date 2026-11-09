#!/usr/bin/env bash
# Changes the version of the server and the client ([workspace.package] version in Cargo.toml)
# and updates Cargo.lock to match.
#
#   scripts/bump-version.sh            show the current version
#   scripts/bump-version.sh patch      1.2.3 -> 1.2.4
#   scripts/bump-version.sh minor      1.2.3 -> 1.3.0
#   scripts/bump-version.sh major      1.2.3 -> 2.0.0
#   scripts/bump-version.sh 1.5.0      set an exact version
#
# Commit the change afterwards; the next build on main uses it for the pre-release.
set -euo pipefail
cd "$(dirname "$0")/.."

# awk instead of a sed block: BSD sed (macOS) rejects `{s/.../p}` without a `;`.
current=$(awk '/^\[workspace\.package\]/ { inside = 1; next } /^\[/ { inside = 0 } inside && /^version = "/ { sub(/^version = "/, ""); sub(/".*$/, ""); print; exit }' Cargo.toml)
if [ -z "$current" ]; then
  echo "No [workspace.package] version found in Cargo.toml" >&2
  exit 1
fi
if [ $# -eq 0 ]; then
  echo "$current"
  exit 0
fi

IFS=. read -r major minor patch <<<"${current%%[-+]*}"
case "$1" in
  major) next="$((major + 1)).0.0" ;;
  minor) next="$major.$((minor + 1)).0" ;;
  patch) next="$major.$minor.$((patch + 1))" ;;
  *)
    if [[ ! "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
      echo "Usage: $0 [major|minor|patch|X.Y.Z]" >&2
      exit 1
    fi
    next="$1"
    ;;
esac

sed -i.bak "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$next\"/" Cargo.toml
rm -f Cargo.toml.bak
# Cargo.lock records the workspace crates' versions too.
cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet
echo "$current -> $next"
