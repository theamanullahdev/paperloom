# Paperloom

Live (video) wallpaper application for Ubuntu and Linux Mint, built primarily for Cinnamon but aiming for general Ubuntu/Mint compatibility.

Think "Wallpaper Engine" / "Lively Wallpaper", but native to the Linux desktop, GUI-only, and engineered to cost effectively nothing in CPU/RAM while idle.

**Status:** 🚧 Early development, but the core app actually works end to end on X11: open the GUI, import an MP4, click "Set as wallpaper" — it plays, full-screen, as your real desktop background, no CLI step required, and automatically pauses on the triggers you've enabled (fullscreen app, screen lock, battery saver, a window covering the desktop). Has its own visual identity now (not Iced's stock theme) and a real animated UI interaction. Requirements/architecture are settled (see [PROJECT.md](./PROJECT.md)). Wayland has a full implementation (layer-shell surface + EGL + libmpv's render API) but **has never run against a real compositor** — this dev machine is X11-only, so it's build-verified only; see `AGENTS.md` and `backends/wayland.rs`'s own doc comment before trusting it.

## Planned highlights
- Play any MP4 as an animated desktop background (short seamless loops included).
- Modern, GUI-first app — no CLI required.
- Aggressively optimized idle behavior (pause/suspend rendering when the wallpaper isn't visible).
- Eventually distributed as a public repo + Flatpak.

Contributing or picking this up as an agent? Start at [AGENTS.md](./AGENTS.md).

## Building

```
sudo apt install libmpv-dev   # required by paperloom-engine
cargo build
cargo run -p paperloom-gui   # Import a wallpaper, click "Set as wallpaper" — done
```

That's the real path: the GUI will start `paperloom-engine` itself the first time you set a wallpaper. For direct engine testing without the GUI:

```
cargo run -p paperloom-engine -- /path/to/loop.mp4   # Ctrl+C to stop
```

`paperloom-gui` builds and runs with no extra system packages. `paperloom-engine` needs the system's mpv client library at build time via the `libmpv2` crate (not the plain `libmpv` crate — see AGENTS.md for why) — `libmpv-dev` above provides it.

## Packaging (.deb)

```
cargo install cargo-deb   # one-time, already present on this machine
cargo build --release --workspace
cargo deb -p paperloom-gui   # produces target/debian/paperloom_<version>_<arch>.deb
```

Builds both binaries into one `paperloom` package (`usr/bin/paperloom-gui`, `usr/bin/paperloom-engine`), plus a desktop entry and icon (`packaging/`) so it shows up in the app menu. Config is in `crates/paperloom-gui/Cargo.toml`'s `[package.metadata.deb]` — shared-library dependencies (`libmpv2`, etc.) are auto-detected from the actual compiled binaries, not hand-maintained.

No Flatpak manifest yet: a real one needs every Cargo dependency vendored for Flatpak's offline sandboxed build (via `flatpak-cargo-generator` against `Cargo.lock`), which is its own significant chunk of work — left for closer to an actual public release rather than attempted half-verified here. `.deb` was achievable and fully buildable in this environment, so that's what exists today; `PROJECT.md` still calls Flatpak the long-term distribution target.

