#!/usr/bin/env bash
# Builds the Flathub manifest for a release: pins the paperloom module to a
# git tag (and its commit), pins the other git modules to their commits, and
# regenerates cargo-sources.json from that tag's Cargo.lock (Flathub builds
# have no network, so every crate must be listed).
#
#   ./update.sh            # newest release tag
#   ./update.sh v0.9.0     # a specific tag
set -euo pipefail
cd "$(dirname "$0")"
repo="$(git rev-parse --show-toplevel)"
app=io.github.theamanullahdev.Paperloom

tag="${1:-$(git -C "$repo" tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1)}"
commit="$(git -C "$repo" rev-parse "$tag^{commit}")"
echo "Pinning to $tag ($commit)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git -C "$repo" show "$tag:Cargo.lock" > "$work/Cargo.lock"
git -C "$repo" show "$tag:packaging/flatpak/$app.yml" > "$work/manifest.yml"

# A throwaway virtual environment: nothing is installed system-wide.
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install --quiet aiohttp tomlkit
curl -fsSL -o "$work/flatpak-cargo-generator.py" \
  https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
"$work/venv/bin/python" "$work/flatpak-cargo-generator.py" "$work/Cargo.lock" -o cargo-sources.json

python3 - "$work/manifest.yml" "$tag" "$commit" > "$app.yml" <<'PY'
import re, subprocess, sys
manifest, tag, commit = open(sys.argv[1]).read(), sys.argv[2], sys.argv[3]

# 1. The app itself: the pinned release instead of the local checkout.
local = re.compile(r"(    sources:\n)      - type: dir\n        path: \.\./\.\.\n(?:        skip:\n(?:          - .*\n)+)?(      - cargo-sources\.json\n)")
def pin(match):
    return (match.group(1) + "      - type: git\n        url: https://github.com/theamanullahdev/paperloom.git\n"
            f"        tag: {tag}\n        commit: {commit}\n"
            "        x-checker-data:\n          type: git\n          tag-pattern: ^(v[\\d.]+)$\n"
            + match.group(2))
manifest, n = local.subn(pin, manifest)
if n != 1:
    sys.exit("could not find the paperloom module's local source block in the manifest")

# 2. Every other git source: add the commit Flathub wants next to the tag.
def add_commit(match):
    url, tag_line = match.group(1), match.group(2)
    name = tag_line.strip().split(": ", 1)[1]
    out = subprocess.run(["git", "ls-remote", url, f"refs/tags/{name}^{{}}", f"refs/tags/{name}"], capture_output=True, text=True, check=True).stdout.split()
    if not out:
        sys.exit(f"tag {name} not found in {url}")
    sha = out[0]
    return f"{match.group(0)}\n        commit: {sha}"
manifest = re.sub(r"      - type: git\n        url: (https://(?!github.com/theamanullahdev)[^\n]+)\n(        tag: [^\n]+)(?!\n        commit:)", add_commit, manifest)
sys.stdout.write(manifest)
PY
printf '{\n  "only-arches": ["x86_64"]\n}\n' > flathub.json
echo "Wrote $app.yml, cargo-sources.json, flathub.json for $tag"
