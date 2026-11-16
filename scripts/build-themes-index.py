#!/usr/bin/env python3
"""Builds themes/index.json, the online library of app themes.

Each folder under themes/ with a theme.toml is one theme. The index lists
every file of it with its SHA-256 and size (the app installs a theme only
if every file matches). Each theme needs a small preview.png thumbnail.

    python3 scripts/build-themes-index.py

Every theme is checked with the app's own checker first
(`cargo run -p paperloom-theme --example check`); a theme with an error or
a warning stops the build.
"""

import hashlib
import json
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
THEMES = ROOT / "themes"
ALLOWED = {".toml", ".ttf", ".otf", ".png", ".txt"}
MAX_PREVIEW = 300 * 1024
MAX_THEME = 12 * 1024 * 1024


def main() -> int:
    folders = sorted(p for p in THEMES.iterdir() if (p / "theme.toml").is_file())
    check = subprocess.run(["cargo", "run", "-q", "-p", "paperloom-theme", "--example", "check", "--", *map(str, folders)], cwd=ROOT, text=True, capture_output=True)
    if check.returncode != 0:
        print(check.stdout + check.stderr, file=sys.stderr)
        return 1

    packs = []
    for folder in folders:
        theme = tomllib.loads((folder / "theme.toml").read_text())
        if theme.get("id") != folder.name:
            print(f"{folder.name}: id in theme.toml must equal the folder name", file=sys.stderr)
            return 1
        preview = folder / "preview.png"
        if not preview.is_file() or preview.stat().st_size > MAX_PREVIEW:
            print(f"{folder.name}: needs a preview.png of at most 300 KB", file=sys.stderr)
            return 1
        files = []
        for path in sorted(p for p in folder.rglob("*") if p.is_file() and p.name != "preview.png"):
            if path.suffix.lower() not in ALLOWED:
                print(f"{folder.name}: {path.name} is not a kind of file a theme may have", file=sys.stderr)
                return 1
            data = path.read_bytes()
            files.append({"path": path.relative_to(folder).as_posix(), "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)})
        if sum(f["size"] for f in files) > MAX_THEME:
            print(f"{folder.name}: larger than 12 MB", file=sys.stderr)
            return 1
        packs.append({"id": theme["id"], "name": theme.get("name", theme["id"]), "author": theme.get("author", ""), "description": theme.get("description", ""), "folder": folder.name, "files": files})

    (THEMES / "index.json").write_text(json.dumps({"format": 1, "packs": packs}, indent=2) + "\n")
    print(f"themes/index.json: {len(packs)} themes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
