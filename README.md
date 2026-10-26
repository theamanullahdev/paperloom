# Paperloom

Live (video) wallpapers for Linux, Windows and macOS. Built first for Linux Mint Cinnamon, with general Ubuntu/Mint compatibility, and now ported to Windows and macOS as well.

Think "Wallpaper Engine" / "Lively Wallpaper": GUI-only, and engineered to cost effectively nothing in CPU/RAM while idle.

Open the app, import an MP4 (or download one from the built-in online library), click **Set as wallpaper**, and it plays full-screen as your real desktop background. It pauses automatically when a fullscreen app is focused, the screen is locked, the battery is low, or a window covers the desktop, so it costs next to nothing when you can't see it. It has a gallery library with collections, an Online tab that searches several wallpaper sites at once, a download manager, and a tray widget.

| Platform | How |
|---|---|
| Linux (X11) | `.deb`, `.rpm`, Flatpak, or from source |
| Linux (Wayland: Hyprland, Sway, …) | wlr-layer-shell background layer |
| Windows 10/11 | setup `.exe` or `.msi`; the window sits behind the desktop icons (including the Windows 11 24H2 layout) |
| macOS 15+ | `.dmg`; desktop-level window on the main display |

## What it does

- **Library:** import MP4, WebM, MKV, MOV, GIF and still images; collections you make yourself plus automatic ones (by topic, source, favorites and tags); favorites, tags and sorting; duplicate imports are recognised.
- **Sharing:** send one wallpaper, a selection, a collection or your whole library to a friend as a short share code or a `.paperloomshare` file; they import it, see a preview, pick what they want and the wallpapers download from the same sites into a collection of their choice. Nothing from your computer is sent, only where each wallpaper comes from. Select several wallpapers with the **Select** button; open a share file by double-clicking it, dropping it on the window or choosing **Import a share**.
- **Cheap to run:** imports are re-encoded in a low-priority background step only when that makes them cheaper to play (formats a graphics card can't decode, more pixels than your screen, more than 30 fps). Hardware decoding, a small decode cache, no audio decoding while muted, and a guard that lowers quality by itself if a wallpaper keeps the CPU busy.
- **Online:** searches several sources at once, hides wallpapers you already have, opens a detail pop-up with a streaming preview, and takes pasted links.
- **Rotation and schedule:** change the wallpaper on an interval or by a weekly schedule, even with the window closed.
- **Pauses by itself:** fullscreen app, locked screen, low battery, covered desktop, chosen apps, or no input for a while.
- **Control:** tray menu, global keyboard shortcuts, and `paperloom-gui next|pause|resume|mute|stop|status|set <name>` from a terminal.
- **Updates:** inside the app, any release (older ones too) can be downloaded and installed.
- **Developer mode:** Settings → Advanced adds a Developers page with live engine status, copyable diagnostics, logs, test actions, and **Run all tests**, which checks everything on a computer and writes one report.

Contributing or picking this up as an agent? Start at [AGENTS.md](./AGENTS.md).

## Install

Download from the [Releases page](https://github.com/theamanullahdev/paperloom/releases/latest):

| System | File |
|---|---|
| Windows | `…-windows-x86_64-setup.exe` (or the `.msi` for managed installs) |
| macOS, Apple Silicon / Intel | `…-macos-aarch64.dmg` / `…-macos-x86_64.dmg` |
| Ubuntu, Linux Mint 22+, Debian | `…-linux-x86_64.deb` |
| Fedora, openSUSE | `…-linux-x86_64.rpm` |
| Any Linux | `…-linux-x86_64.flatpak` (`flatpak install paperloom-….flatpak`) |

Once installed, Paperloom keeps itself current. **Settings → Updates** checks GitHub for new releases, downloads the right installer for your system (resumable, SHA-256 verified) and opens it. You can also skip a version there. Flatpak installs are updated by Flatpak instead.

### Ubuntu / Linux Mint / Debian: install from the apt repository (recommended)

This gets you updates automatically with your normal system updates (Update Manager / `sudo apt upgrade`):

```
sudo curl -fsSLo /usr/share/keyrings/paperloom.asc https://theamanullahdev.github.io/paperloom/apt/paperloom.asc
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/paperloom.asc] https://theamanullahdev.github.io/paperloom/apt stable main" | sudo tee /etc/apt/sources.list.d/paperloom.list
sudo apt update && sudo apt install paperloom
```

If you already installed a `.deb` by hand, run the same three lines: apt takes over that install and keeps it updated from then on. The repository is published by `.github/workflows/apt-repo.yml` on every release and signed with the `APT_GPG_PRIVATE_KEY` secret (setup is described at the top of that file).

### Other Linux distros (Arch, Manjaro, EndeavourOS, Hyprland, Sway, …)

**Easiest, any distro: Flatpak.** Download `…-linux-x86_64.flatpak` from the [Releases page](https://github.com/theamanullahdev/paperloom/releases/latest), then:

```
flatpak install --user ./paperloom-*-linux-x86_64.flatpak
flatpak run io.github.theamanullahdev.Paperloom
```

(Needs `flatpak` installed, e.g. `sudo pacman -S flatpak`. Flatpak installs are updated through Flatpak.)

**Arch / Manjaro / EndeavourOS, from source with pacman tooling.** There's a `PKGBUILD` in [`packaging/arch`](./packaging/arch):

```
sudo pacman -S --needed base-devel git
git clone https://github.com/theamanullahdev/paperloom && cd paperloom/packaging/arch
makepkg -si
```

**Any distro, build from source.** You need Rust (`rustup` or your distro's `cargo`), `pkg-config`, and mpv's library with headers, plus `ffmpeg` at runtime (for hover previews):

| Distro | Install the build/run dependencies |
|---|---|
| Arch and derivatives | `sudo pacman -S --needed rust mpv ffmpeg pkgconf libxkbcommon wayland vulkan-icd-loader` |
| Debian, Ubuntu, Mint | `sudo apt install cargo pkg-config libmpv-dev ffmpeg libxkbcommon-dev` |
| Fedora | `sudo dnf install cargo pkgconf mpv-devel ffmpeg-free libxkbcommon-devel` |
| openSUSE | `sudo zypper install cargo pkgconf libmpv-devel ffmpeg libxkbcommon-devel` |

```
git clone https://github.com/theamanullahdev/paperloom && cd paperloom
cargo build --release
sudo install -Dm755 target/release/paperloom-gui target/release/paperloom-engine -t /usr/local/bin
paperloom-gui
```

Both programs must be installed in the same folder (the GUI starts the engine from next to itself).

**Hyprland, Sway and other wlroots compositors (Wayland).** Paperloom draws the wallpaper on a `wlr-layer-shell` background layer, which these compositors provide. Wayland has no portable way to see fullscreen or covering windows, so there the wallpaper pauses for screen lock and low battery. The tray icon needs a bar with StatusNotifier support (for Waybar, enable the `tray` module); otherwise use the app window. Found a problem? [Open an issue](https://github.com/theamanullahdev/paperloom/issues).

**Software centers:** Linux Mint's Software Manager and GNOME Software list apps from your enabled sources. After adding the apt repository above, `paperloom` is searchable in Mint's Software Manager like any other package.

### Updating otherwise

**Settings → Updates** in the app downloads the newest installer and opens it in your system's package installer, where you confirm. A `.deb`/`.rpm` downloaded by hand doesn't update in the background. To update one from a terminal:

```
curl -fsSL https://api.github.com/repos/theamanullahdev/paperloom/releases/latest \
  | grep -o 'https://[^"]*linux-x86_64\.deb' | head -n1 | xargs curl -fL -o /tmp/paperloom.deb \
  && sudo apt install /tmp/paperloom.deb
```

On Fedora, replace `\.deb` with `\.rpm` and the last line with `sudo dnf install /tmp/paperloom.rpm`. Restart Paperloom (and stop/start the wallpaper) afterwards.

**First launch on Windows and macOS:**

- **Windows:** SmartScreen shows "Windows protected your PC". Click **More info → Run anyway**.
- **macOS:** open **System Settings → Privacy & Security** and click **Open Anyway** next to Paperloom, or run `xattr -dr com.apple.quarantine /Applications/Paperloom.app`.

Linux requirements: the `.deb`/`.rpm` need the system's libmpv (`libmpv2` on Ubuntu 24.04 / Mint 22; `mpv-libs` on Fedora), which the package manager installs automatically. Ubuntu 22.04 / Mint 21 ship an mpv that's too old (client API v1).

## Releases

A release is started on purpose from the Actions tab and becomes a new [GitHub Release](https://github.com/theamanullahdev/paperloom/releases), built by `.github/workflows/release.yml`:

1. **Version:** worked out from the commit messages since the last release (`scripts/next-version.sh`):
   - `feat: …` → minor (0.**2**.0)
   - `fix: …` or anything else → patch (0.1.**1**)
   - `feat!: …` or `BREAKING CHANGE` → major (minor while the version is still 0.x)
2. **Test:** clippy and the test suite.
3. **Build every installer:** `.deb`, `.rpm`, `.flatpak`, `.msi`, setup `.exe`, and both `.dmg`s, each on its real platform (`package.yml`).
4. **Publish, only if all of that passed:** commit the version bump, tag `vX.Y.Z`, and create the Release. It carries every installer, `SHA256SUMS.txt`, and notes listing the commits.

Pushing commits never releases. To cut a release, open **Actions → Release → Run workflow** (or `gh workflow run release.yml`) and leave the bump on auto or pick patch, minor or major.

CI (`ci.yml`) runs on every push and PR. It builds all the same installers without publishing them, and they're downloadable from that run's **Artifacts** section.

If you ever add branch protection to `main`, allow the `github-actions[bot]` to push (or the release's version-bump commit will only live on the tag; releases still work).

## Flathub

The Flathub submission files (manifest, pinned sources, screenshots and the steps) are in [`flathub/`](./flathub).

## Building from source

### Linux

```
sudo apt install libmpv-dev ffmpeg   # libmpv for the engine, ffmpeg for gallery previews
cargo build
cargo run -p paperloom-gui            # Import a wallpaper, click "Set as wallpaper" — done
```

The GUI starts `paperloom-engine` itself the first time you set a wallpaper. To test the engine directly, without the GUI:

```
cargo run -p paperloom-engine -- /path/to/loop.mp4   # Ctrl+C to stop
```

The engine links the system's mpv client library through the `libmpv2` crate. It's not the plain `libmpv` crate; AGENTS.md explains why. Packages:

```
cargo install cargo-deb cargo-generate-rpm
cargo build --release --workspace
cargo deb -p paperloom-gui --no-build          # target/debian/*.deb
cargo generate-rpm -p crates/paperloom-gui     # target/generate-rpm/*.rpm
```

### Windows

Run these in an MSVC developer PowerShell, with `gh` and `7z` installed:

```
pwsh packaging/windows/fetch-mpv.ps1 -Out C:\mpv   # libmpv-2.dll, ffmpeg.exe, mpv.lib
$env:RUSTFLAGS = "-L native=C:\mpv"
cargo build --release --workspace
```

Copy `libmpv-2.dll` and `ffmpeg.exe` next to the built executables. The installers are built with WiX 5 (`packaging/windows/paperloom.wxs`) and Inno Setup 6 (`paperloom.iss`); see `package.yml` for the exact commands.

### macOS

```
brew install mpv ffmpeg dylibbundler
RUSTFLAGS="-L $(brew --prefix)/lib" cargo build --release --workspace
packaging/macos/build-dmg.sh 0.1.0 target/release dist   # bundles every dylib into Paperloom.app
```
