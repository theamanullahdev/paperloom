#!/usr/bin/env bash
# Writes a release's notes (Markdown) to stdout: a download table built from
# the installer files actually present, then the commits since the previous
# release.
#
#   scripts/release-notes.sh VERSION DIST_DIR early|final
#
# "early": the Linux packages are out and the rest are still building, so
# missing rows say they're on the way. "final": every build has finished,
# so a missing row means that build failed.
set -euo pipefail
version="$1"; dist="$2"; stage="$3"
cd "$(dirname "$0")/.."

missing=()
row() { # $1 = label, $2 = file name
  if [ -f "$dist/$2" ]; then echo "| $1 | \`$2\` |"; else missing+=("$1"); fi
}

echo "## Download"
echo
echo "| System | File |"
echo "|---|---|"
row "Ubuntu / Linux Mint / Debian" "paperloom-$version-linux-x86_64.deb"
row "Fedora / openSUSE" "paperloom-$version-linux-x86_64.rpm"
row "Windows (installer)" "paperloom-$version-windows-x86_64-setup.exe"
row "Windows (MSI, for managed installs)" "paperloom-$version-windows-x86_64.msi"
row "macOS, Apple Silicon" "paperloom-$version-macos-aarch64.dmg"
row "macOS, Intel" "paperloom-$version-macos-x86_64.dmg"
row "Any Linux (Flatpak bundle)" "paperloom-$version-linux-x86_64.flatpak"
echo
if [ "${#missing[@]}" -gt 0 ]; then
  list="$(printf '%s; ' "${missing[@]}")"; list="${list%; }"
  if [ "$stage" = early ]; then
    echo "Still building, and added here automatically in a few minutes: $list."
  else
    echo "Not in this release (that build failed; it will be in the next one): $list."
  fi
  echo
fi
echo "Using the apt repository? This version arrives with your normal system updates. Otherwise Paperloom offers it under **Settings → Updates**."
echo "The installers aren't code-signed yet, so Windows SmartScreen and macOS Gatekeeper warn on first launch — see the README for how to get past that."
echo
echo "## Changes"
echo
previous="$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | grep -vx "v$version" | head -n1 || true)"
git log --format='- %s (%h)' "${previous:+$previous..}v$version" --invert-grep --grep='^chore(release):' 2>/dev/null \
  || git log --format='- %s (%h)' "${previous:+$previous..}HEAD" --invert-grep --grep='^chore(release):'
