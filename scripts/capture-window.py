#!/usr/bin/env python3
"""Capture an X11 window's actual rendered content, by window ID.

Unlike a normal screenshot tool, this reads the window's content directly
via XGetImage rather than whatever's currently visible on screen — so it
works correctly even when the window is fully covered by other windows,
as long as a compositor is running (Cinnamon/Muffin keeps every window
rendered to its own off-screen buffer regardless of visibility). Useful
for verifying paperloom-engine's desktop-layer window, which sits behind
every normal window by design and is often covered by whatever else is
open.

Usage:
    python3 scripts/capture-window.py <window_id> <output.png>

Find the window id with: wmctrl -lG   (paperloom-engine's window has no
title — look for the one named "N/A" matching your screen resolution).

Requires: python3-xlib, Pillow (both already present on this machine).
"""

import sys

from PIL import Image
from Xlib import X, display


def main() -> None:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <window_id> <output.png>", file=sys.stderr)
        raise SystemExit(1)

    window_id = int(sys.argv[1], 0)
    out_path = sys.argv[2]

    d = display.Display()
    win = d.create_resource_object("window", window_id)
    geom = win.get_geometry()

    raw = win.get_image(0, 0, geom.width, geom.height, X.ZPixmap, 0xFFFFFFFF)
    img = Image.frombytes("RGB", (geom.width, geom.height), raw.data, "raw", "BGRX")
    img.save(out_path)
    print(f"saved {geom.width}x{geom.height} -> {out_path}")


if __name__ == "__main__":
    main()
