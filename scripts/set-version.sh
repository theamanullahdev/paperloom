#!/usr/bin/env bash
# Sets the workspace version everywhere it's recorded: Cargo.toml,
# Cargo.lock (workspace crates only) and the Flatpak metainfo's release list.
#
#   scripts/set-version.sh 0.3.0
set -euo pipefail
cd "$(dirname "$0")/.."
version="$1"
today="$(date -u +%Y-%m-%d)"

awk -v v="$version" '
  /^\[workspace.package\]/ { s = 1 }
  /^\[/ && !/^\[workspace.package\]/ { s = 0 }
  s && /^version *=/ { $0 = "version = \"" v "\"" }
  { print }
' Cargo.toml > Cargo.toml.new && mv Cargo.toml.new Cargo.toml

# Workspace members carry the workspace version in the lock file.
for crate in paperloom-common paperloom-engine paperloom-fetch paperloom-gui paperloom-widgets; do
  awk -v c="$crate" -v v="$version" '
    $0 == "name = \"" c "\"" { hit = 1; print; next }
    hit && /^version = / { $0 = "version = \"" v "\""; hit = 0 }
    { print }
  ' Cargo.lock > Cargo.lock.new && mv Cargo.lock.new Cargo.lock
done

metainfo=packaging/flatpak/io.github.theamanullahdev.Paperloom.metainfo.xml
if ! grep -q "release version=\"$version\"" "$metainfo"; then
  sed -i.bak "s|<releases>|<releases>\n    <release version=\"$version\" date=\"$today\" />|" "$metainfo" && rm -f "$metainfo.bak"
fi
