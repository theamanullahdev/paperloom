#!/usr/bin/env python3
"""Rebuilds widgets/index.json: the catalogue the app's Browse → Online reads.

Every folder under widgets/ with a skin.toml is one look. The index lists its
files with SHA-256 checksums and sizes (the app verifies each file before it
installs anything). With --previews it first redraws every folder's
preview.png using the app's own renderer, so previews can't drift from what
the app draws.

    scripts/build-widgets-index.py [--previews]
"""
import hashlib
import json
import pathlib
import subprocess
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent
widgets = root / "widgets"

folders = sorted(p for p in widgets.iterdir() if (p / "skin.toml").is_file())

if "--previews" in sys.argv:
    subprocess.run(["cargo", "build", "-q", "-p", "paperloom-skin", "--example", "render"], cwd=root, check=True)
    for folder in folders:
        subprocess.run([str(root / "target/debug/examples/render"), str(folder), str(folder / "preview.png")], check=True)

packs = []
for folder in folders:
    skin = tomllib.loads((folder / "skin.toml").read_text())
    files = []
    for path in sorted(p for p in folder.rglob("*") if p.is_file() and p.name != "preview.png"):
        data = path.read_bytes()
        files.append({"path": path.relative_to(folder).as_posix(), "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
    packs.append({
        "id": skin["id"],
        "name": skin["name"],
        "author": skin.get("author", ""),
        "description": skin.get("description", ""),
        "kinds": skin["kinds"],
        "folder": folder.name,
        "files": files,
    })

(widgets / "index.json").write_text(json.dumps({"format": 1, "packs": packs}, indent=2) + "\n")
print(f"{len(packs)} looks, {sum(f['size'] for p in packs for f in p['files']) // 1024} KB")
