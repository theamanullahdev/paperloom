#!/usr/bin/env bash
# Decides the next release version from the commits since the last release
# tag, following Conventional Commits loosely:
#   - "BREAKING CHANGE" or "type!:"  -> major (minor while still 0.x)
#   - "feat:" / "feat(scope):"       -> minor
#   - anything else                  -> patch
# Prints nothing when nothing release-worthy happened: no new commits, or
# they only touched docs (*.md, docs/, site/). A forced bump always prints.
#
#   scripts/next-version.sh [auto|patch|minor|major]
#
# Release tags point at a version-bump commit made on top of the built
# commit (see release.yml), which may never land on main if main moved on
# in the meantime — so the last tag is found by version order, not by
# `git describe`, and "since the last tag" is `tag..HEAD`.
set -euo pipefail
cd "$(dirname "$0")/.."
force="${1:-auto}"

last_tag="$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1)"
if [ -z "$last_tag" ]; then
  # Never released: the first release is whatever Cargo.toml says.
  scripts/version.sh
  exit 0
fi

range="$last_tag..HEAD"
commits="$(git log --format=%H "$range" --invert-grep --grep='^chore(release):')"
changed="$(git log --format= --name-only "$range" --invert-grep --grep='^chore(release):' | sort -u | grep -Ev '(\.md$|^docs/|^site/)' || true)"
if [ "$force" = "auto" ] && { [ -z "$commits" ] || [ -z "$changed" ]; }; then
  exit 0
fi

messages="$(git log --format='%s%n%b' "$range" --invert-grep --grep='^chore(release):')"
IFS=. read -r major minor patch <<<"${last_tag#v}"
patch="${patch%%-*}"

bump="$force"
if [ "$bump" = "auto" ]; then
  if echo "$messages" | grep -Eq '^BREAKING[ -]CHANGE|^[a-z]+(\([^)]*\))?!:'; then
    # Before 1.0 a breaking change is a minor bump (semver's 0.x convention).
    if [ "$major" = "0" ]; then bump=minor; else bump=major; fi
  elif echo "$messages" | grep -Eq '^feat(\([^)]*\))?:'; then
    bump=minor
  else
    bump=patch
  fi
fi

case "$bump" in
  major) echo "$((major + 1)).0.0" ;;
  minor) echo "$major.$((minor + 1)).0" ;;
  patch) echo "$major.$minor.$((patch + 1))" ;;
  *) echo "unknown bump: $bump" >&2; exit 1 ;;
esac
