#!/usr/bin/env bash
# Prints the workspace version from the root Cargo.toml ([workspace.package]).
set -euo pipefail
cd "$(dirname "$0")/.."
awk '/^\[workspace.package\]/{s=1;next} /^\[/{s=0} s && /^version *=/{gsub(/"/,"",$3); print $3; exit}' Cargo.toml
