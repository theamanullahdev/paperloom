#!/usr/bin/env python3
"""Makes the files behind Paperloom's small updates (Windows setup installs).

    make-update-manifest.py <stage dir> <version> <out dir> <download base url> [--previous manifest.json]

Writes into <out dir>:
  paperloom-<version>-windows-x86_64-manifest.json   every file of the install: name, SHA-256, size, packed size, URL
  paperloom-file-<sha256>.gz                          each file, gzip-compressed, named by its checksum

A file that is byte-for-byte the same as in the previous release's manifest is not
uploaded again: its entry keeps pointing at the earlier release's copy. The app
compares the manifest with what is installed and downloads only the differences,
checking each against its checksum (crates/paperloom-fetch/src/delta.rs).
"""
import gzip
import hashlib
import json
import pathlib
import shutil
import sys


def main() -> None:
    args = sys.argv[1:]
    previous = None
    if "--previous" in args:
        at = args.index("--previous")
        previous = json.loads(pathlib.Path(args[at + 1]).read_text())
        del args[at : at + 2]
    stage, version, out, base = pathlib.Path(args[0]), args[1], pathlib.Path(args[2]), args[3].rstrip("/")
    out.mkdir(parents=True, exist_ok=True)

    earlier = {(f["path"], f["sha256"]): f for f in (previous or {}).get("files", [])}
    files = []
    uploaded = reused = 0
    for path in sorted(p for p in stage.iterdir() if p.is_file()):
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        known = earlier.get((path.name, digest))
        if known:
            files.append({**known})
            reused += 1
            continue
        name = f"paperloom-file-{digest}.gz"
        target = out / name
        with gzip.GzipFile(target, "wb", compresslevel=9, mtime=0) as packed:
            packed.write(data)
        files.append({
            "path": path.name,
            "sha256": digest,
            "size": len(data),
            "packed": target.stat().st_size,
            "url": f"{base}/{name}",
        })
        uploaded += 1

    manifest = {"format": 1, "version": version, "files": files}
    (out / f"paperloom-{version}-windows-x86_64-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    total = sum(f["packed"] for f in files)
    print(f"{len(files)} files ({total // 1024 // 1024} MB packed): {uploaded} new, {reused} unchanged since the last release")


if __name__ == "__main__":
    main()
