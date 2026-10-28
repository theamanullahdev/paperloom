# Project Requirements & Architecture Log

Living document. Update this as decisions are made — don't let it drift from reality. For current implementation status (what's actually built vs. stubbed), see `AGENTS.md` — this file is requirements/decisions, not a build log.

## 1. Pitch
A live wallpaper application, primarily targeting Linux Mint Cinnamon, generally compatible with Ubuntu and other Mint editions — and, since 2026-10-18, also Windows and macOS (user decision: "Windows/macOS is a real goal"). Plays looping MP4 video files as the desktop background, the way Wallpaper Engine / Lively Wallpaper do on Windows.

## 2. Hard requirements (non-negotiable)
- **GUI only.** No CLI-driven workflow. The app must be usable entirely through a graphical interface.
- **Near-zero idle footprint.** When no wallpaper is actively rendering (or in whatever "idle" states we define), CPU and RAM usage should be effectively negligible. This is the top architectural constraint and should override convenience/familiarity when the two conflict.
- **Modern UI.** Clean, contemporary design — macOS-style polish as the bar, not stock GTK defaults.
- **Input format:** must play arbitrary MP4 files the user supplies (e.g. downloaded from live-wallpaper sites), typically short (3-4s) seamless loops.
- **Target DE:** primary = Linux Mint Cinnamon. Secondary = Ubuntu (GNOME) and other Mint flavors (XFCE, MATE) on a best-effort basis.
- **Distribution goal:** public GitHub repo with fully automatic GitHub Releases carrying `.deb`, `.rpm`, `.flatpak` (Linux), `.msi` + setup `.exe` (Windows) and `.dmg` (macOS); Flathub submission when ready. No AppImage (user decision). The app updates itself from those releases (Settings → Updates).

## 3. Likely feature set (draft — confirm/expand)
- Browse/pick an MP4 and set it as the live wallpaper.
- Per-monitor wallpaper assignment (multi-monitor support).
- Playback controls: volume/mute (most live wallpapers are silent), loop behavior.
- Library/gallery view of saved wallpapers.
- Auto-pause triggers (draft, pending confirmation): fullscreen app focused, screen locked, battery saver active, high GPU load from another app.
- "Optimizer": adaptive quality/frame-rate scaling based on system load or on-battery state.
- Playlist/rotation of multiple wallpapers (maybe later).
- **Web catalog/discovery page — planned, post-v1.** A separate page inside the app where the user configures source URLs (e.g. Pexels and other free stock-video sites) and the app fetches/browses wallpapers directly from them. **v1 scope is local-file import only** — this is explicitly deferred, not built now.

## 4. Open questions
Resolved (see Section 5 for rationale):
- [x] Display server → both X11 and Wayland, from day one.
- [x] UI toolkit → Iced.
- [x] Video backend → libmpv.
- [x] Idle/pause triggers → all of them, but user-configurable (see Section 6).

Still open (not blocking stack choice, decide before v1 feature-freeze):
- [x] Packaging → **.deb, .rpm, .flatpak, .msi, setup .exe, .dmg** (user decision 2026-10-18; no AppImage), all built in CI by `.github/workflows/package.yml` and published automatically by `release.yml` (version from Conventional Commits). Unsigned for now — code signing (Apple Developer ID / Windows certificate) is a later decision that needs the user's accounts.
- [x] License → GPL-3.0-or-later (`LICENSE`).
- [ ] Multi-monitor: required for v1 or later? (Note: X11's current implementation already *happens* to mirror across every monitor, as a side effect of the root window spanning the whole X11 screen — not a deliberate per-monitor feature. Wayland's `backends::wayland.rs` does not do this — it binds to a single output only, a known limitation noted in that file.)
- [ ] Audio support in MP4s: muted always, user-toggleable, or per-wallpaper setting? → Leaning confirmed and **implemented**: muted by default (`Settings.muted`), global toggle in the Settings page, read by the engine on startup and on every `Play`. Not yet per-wallpaper (global only).
Resolved (continued):
- [x] **Project name → Paperloom.** ("Backdropper" rejected as not distinctive enough. Checked via web search before locking in — no existing software found under this name, unlike earlier rejected candidates "Thrum"/"Verve" which were already taken.)
- [x] Search scope → two separate local-only systems: in-app settings/command search (alias-aware, see Section 6) and local library filename search. A web-based discovery catalog is planned but deferred — see Section 3.

## 5. Stack & core technology decisions

- **Language: Rust.** Confirmed. Keeps the whole stack in one language/toolchain (no cross-runtime overhead), and the two libraries below both have mature Rust bindings.

- **Display server: both X11 and Wayland from day one.** This is real added scope, not free — it means two separate desktop-integration paths sharing one renderer/decoder core:
  - X11: root-window / `xwinwrap`-style embedding.
  - Wayland: `wlr-layer-shell` background layer (the approach `mpvpaper` uses).

- **UI toolkit: Iced** (pure Rust, MIT, `wgpu`-based). Chosen for maximum custom-styling control with no optimization tradeoff:
  - Rejected GTK4/libadwaita — native and light, but fights against a bespoke macOS-like look.
  - Rejected Qt/QML — capable, but drags in a second (C++) toolkit alongside Rust.
  - Rejected Tauri/WebView — easiest to style, but a WebView's idle memory cost directly conflicts with the zero-idle-RAM requirement.
  - Bonus: Iced's `wgpu` context can potentially be shared with the video-rendering pipeline instead of running two separate graphics stacks, which itself helps the resource-footprint goal.

- **Video backend: libmpv**, via the Rust crate **`libmpv2`** specifically (not the older `libmpv` crate — that one vendors mpv client API v1 headers and hard-fails at runtime against current mpv's API v2, confirmed by actually running it; `libmpv2` is the maintained fork targeting v2). Not a default guess otherwise — libmpv is the proven approach for exactly this problem on Linux. `mpvpaper` (Wayland, via `wlr-layer-shell`) and `xwinwrap` + mpv (X11) already demonstrate "video as desktop wallpaper" using mpv, including hardware-accelerated decode (VA-API/VDPAU), which is central to hitting the low-resource/optimizer goal. GStreamer was the alternative but has less direct precedent for this specific use case. **Validated, not just decided:** `paperloom-engine` actually plays a video as the live X11 desktop background today (see AGENTS.md).

## 6. Design philosophy: settings

Nothing is hardcoded as "the one true way to run this." The optimizer/idle-pause behavior described above ships *on* by default — that's the opinionated, battery-and-RAM-friendly default experience — but every knob behind it is exposed in Settings, with no ceiling. A user who doesn't care about resource usage can crank resolution/frame rate/effects all the way up and run multiple unoptimized wallpapers at once ("I don't care, let it burn resources") — that's a valid, fully supported configuration, not a mistake the app should prevent. Default = efficient. Ceiling = uncapped. The settings page and its search (see below) need to be comprehensive enough that this is actually true in practice, not just in principle.

### Search systems (both local-only, and distinct from each other)

1. **Settings/command search.** A search over the app's own UI/options — not a file search. Typing "theme" or "dark mode" must both resolve to the same Appearance/Theme setting and jump straight to the page/section containing it (ideally focusing the control itself). This requires every setting to carry a curated list of search aliases/synonyms, not just a literal match on its displayed label — same idea as macOS System Settings search or VS Code's command palette. This is what makes "everything is a setting" (Section 6, above) actually discoverable rather than just technically true.
2. **Library search.** Fuzzy search across the user's own imported wallpaper files — filenames for v1, with tags/folders as likely future metadata once the library model exists.

A web-based discovery/catalog search (browsing wallpapers from external sites) is a separate, later feature — see Section 3.

## 7. Storage model

**Decision: app-managed library folder by default, with a "reference in place" option for advanced users.**

Default behavior: when a user adds an MP4, the app copies it into its own managed library directory (e.g. `~/.local/share/<app>/library/`) and can run its own optimization pass on the copy (re-encode/transcode for efficient hardware decode, verify/fix seamless looping, generate a thumbnail/preview). Rationale:
- The app can guarantee the file won't vanish or move out from under it (unlike a path into a random Downloads folder or an external drive).
- Optimization (re-encode, loop-trimming, thumbnailing) needs a stable place to put its output anyway.
- This is the same pattern Wallpaper Engine/Lively use on Windows.

Tradeoff: doubles disk usage for that file (original + managed copy) unless the original is deleted/moved after import.

Because of that tradeoff, this is a **setting, not a mandate** — per the philosophy above. Import mode should itself be configurable:
- **Copy & Optimize** (default) — described above.
- **Copy, no re-encode** — duplicate into the library folder but skip transcoding (faster import, keeps original quality/codec as-is).
- **Reference in place** — don't copy at all, just store the path the user gave us; no guaranteed optimization, and it breaks if the user moves/deletes/unmounts the source. Opt-in, clearly labeled with that risk.

## 8. Architecture notes

- **Process model:** a split architecture (tiny always-on control/daemon piece + a renderer that only consumes real resources while a wallpaper must actually be visible) is likely necessary to hit "zero idle RAM" — a single monolithic GUI process sitting in memory may already violate it. To be validated once X11/Wayland integration specifics are prototyped.
- **Idle/pause behavior is config-driven, not hardcoded.** Ship with the most aggressive power-saving defaults enabled — pause on: fullscreen app focused, screen locked, battery saver active, desktop fully covered by another window. Every trigger is an individual toggle in settings, so a user can choose to keep rendering regardless of resource cost if that's what they want. Base/default experience optimizes for minimal usage; the user decides if they want to trade that away.
- Need to confirm how Cinnamon/Mint expects a "wallpaper" to be set (root window vs. compositor layer) to validate the X11 integration path above.

## 9. Non-goals (for now — confirm)
- Image/GIF/shader-based wallpapers (unless later requested) — MP4 is the explicit initial format.

## 10. Desktop widgets (decided 2026-10-29)

Clock, date, system meters and the like on the desktop, in the spirit of Rainmeter. Decisions from the maintainer:

- **Separate process:** a new `paperloom-widgets` daemon with its own windows. The wallpaper engine never draws widgets and is unaffected by them. Widgets sit visibly on top of the wallpaper.
- **Platforms:** one backend each for X11, Windows, Wayland and macOS in the first design; X11 and Windows are brought up first.
- **Off by default.** Enabled in Settings → Widgets (master switch, start at login). The tray icon's menu gets a Widgets on/off toggle; there is no second tray icon.
- **Same pause rules as the wallpaper:** fullscreen app, screen lock, battery, window covering the desktop (shared detector in `paperloom-common`), each a setting.
- **Every built-in widget ships in the catalogue.** A widget that is not placed costs nothing; only placed ones run.
- **Placement:** drag a widget on the desktop itself, or move it on the app's Edit layout page; size, opacity and monitor per widget. Each widget has two switches, both on by default: *drag on the desktop* and *right-click menu* (settings, lock position, remove, hide all). A widget that takes input sits above the desktop icons (their window would otherwise swallow every click); with both off it is fully click-through and sits below the icons.
- **Own widgets:** declarative widget packs (a folder with a layout file and assets, using built-in data sources: time, CPU, memory, disk, network, battery, weather, wallpaper state, text file). No script engine, no web view. A pack that reads a command's output asks for that permission explicitly. Animated widgets are a later addition.
- **Online catalogue is a hard requirement of the first release:** packs live in the repository under `widgets/`; CI publishes `widgets/index.json` and the pack files to GitHub Pages next to `releases.json`; the app's **Widgets → Browse** reads it. Sharing as a `.paperloomwidget` file (same idea as `.paperloomshare`).
- **Weather:** location comes from the system where it offers one (geoclue, Windows, macOS), with a lookup by network address as fallback and a manual override. Open-Meteo, no key.
- **Look:** built-in widgets use Paperloom's theme tokens for now; full per-widget styling comes later.
- **UI:** new top-level **Widgets** page with sub-pages (My widgets, Browse, Layout). Per-widget settings open in a workspace pane (list left, settings right, like Updates), never one long scroll.
- **Budget:** under about 1% CPU and 30 MB with three widgets placed, measured on the Usage page and in `tools/bench`. Redraw only while visible, once a second at most for clocks, every 2 s for meters. Software renderer (tiny-skia + softbuffer), not the Iced/wgpu stack.

Widget safety rules (details in PLAN.md "edge cases"): above the wallpaper, below the desktop icons and below windows, click-through by default, never takes focus; never lose a widget (clamp to the work area, Find my widgets, Reset layout, edit mode always has an exit); coalesced wall-clock ticks and draw-on-change only, with slower rates on battery; packs are declarative and validated, commands need explicit permission; the daemon runs only while at least one widget is placed; the updater ends it before an installer runs.
