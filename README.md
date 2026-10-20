# Paperloom

Live (video) wallpapers for Linux, Windows and macOS. Built first for Linux Mint Cinnamon, with general Ubuntu/Mint compatibility, and now ported to Windows and macOS as well.

Think "Wallpaper Engine" / "Lively Wallpaper": GUI-only, and engineered to cost effectively nothing in CPU/RAM while idle.

**Status:** 🚧 Early development. On **Linux X11** the core app works end to end: open the GUI, import an MP4, click "Set as wallpaper", and it plays full-screen as your real desktop background. It pauses automatically on the triggers you've enabled (fullscreen app, screen lock, low battery, a window covering the desktop). It has a gallery library and an Online tab that browses and downloads wallpapers.

How well each platform is verified:

| Platform | State |
|---|---|
| Linux, X11 | Works, verified on a real desktop |
| Linux, Wayland | Full implementation (layer-shell + EGL + libmpv render API), never run on a real compositor |
| Windows 10/11 | Full implementation (window behind the desktop icons, incl. the Windows 11 24H2 layout), built and packaged in CI, **never run on a real Windows machine yet** |
| macOS 15+ | Full implementation (desktop-level window), built and packaged in CI, **never run on a real Mac yet**; main display only |

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

(Needs `flatpak` installed, e.g. `sudo pacman -S flatpak`. Inside Flatpak, hover previews are missing because ffmpeg's command-line tool isn't bundled, and the in-app updater is off: update by installing the newer `.flatpak`.)

**Arch / Manjaro / EndeavourOS, from source with pacman tooling.** There's a `PKGBUILD` in [`packaging/arch`](./packaging/arch) (untested on a real Arch machine, so treat it as a starting point):

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

**Hyprland, Sway and other wlroots compositors (Wayland).** Paperloom draws the wallpaper on a `wlr-layer-shell` background layer, which these compositors support, but **the Wayland engine has never run on a real compositor yet**, so expect rough edges and please report what happens. Wayland has no window-covering/fullscreen detection, so the wallpaper only pauses for screen lock and low battery there. The tray icon needs a bar with StatusNotifier support (for Waybar, enable the `tray` module); without one, use the app window.

**Software centers:** Linux Mint's Software Manager and GNOME Software list apps from your enabled sources. After adding the apt repository above, `paperloom` is searchable in Mint's Software Manager as an ordinary package. It is **not on Flathub yet**, so it doesn't show up in the Flathub catalog until it is submitted and accepted there.

### Updating otherwise

**Settings → Updates** in the app downloads the newest installer and opens it in your system's package installer, where you confirm. A `.deb`/`.rpm` downloaded by hand doesn't update in the background. To update one from a terminal:

```
curl -fsSL https://api.github.com/repos/theamanullahdev/paperloom/releases/latest \
  | grep -o 'https://[^"]*linux-x86_64\.deb' | head -n1 | xargs curl -fL -o /tmp/paperloom.deb \
  && sudo apt install /tmp/paperloom.deb
```

On Fedora, replace `\.deb` with `\.rpm` and the last line with `sudo dnf install /tmp/paperloom.rpm`. Restart Paperloom (and stop/start the wallpaper) afterwards.

**The installers aren't code-signed yet** (that costs money: an Apple Developer account at $99/year and a Windows code-signing certificate). Until then:

- **Windows:** SmartScreen says "Windows protected your PC". Click **More info → Run anyway**.
- **macOS:** the first launch is blocked. Open **System Settings → Privacy & Security** and click **Open Anyway** next to Paperloom. Alternatively, run `xattr -dr com.apple.quarantine /Applications/Paperloom.app`.

Linux requirements: the `.deb`/`.rpm` need the system's libmpv (`libmpv2` on Ubuntu 24.04 / Mint 22; `mpv-libs` on Fedora), which the package manager installs automatically. Ubuntu 22.04 / Mint 21 ship an mpv that's too old (client API v1).

## Releases (fully automatic)

There is nothing to do by hand. Every push to `main` that changes more than documentation becomes a new [GitHub Release](https://github.com/theamanullahdev/paperloom/releases), built by `.github/workflows/release.yml`:

1. **Version:** worked out from the commit messages since the last release (`scripts/next-version.sh`):
   - `feat: …` → minor (0.**2**.0)
   - `fix: …` or anything else → patch (0.1.**1**)
   - `feat!: …` or `BREAKING CHANGE` → major (minor while the version is still 0.x)
2. **Test:** clippy and the test suite.
3. **Build every installer:** `.deb`, `.rpm`, `.flatpak`, `.msi`, setup `.exe`, and both `.dmg`s, each on its real platform (`package.yml`).
4. **Publish, only if all of that passed:** commit the version bump, tag `vX.Y.Z`, and create the Release. It carries every installer, `SHA256SUMS.txt`, and notes listing the commits.

Pushes that only touch `*.md`/`docs/` don't release. Neither does a commit message containing `[skip release]`. To force a release, open **Actions → Release → Run workflow** and pick patch, minor or major.

CI (`ci.yml`) runs on every push and PR. It builds all the same installers without publishing them, and they're downloadable from that run's **Artifacts** section.

If you ever add branch protection to `main`, allow the `github-actions[bot]` to push (or the release's version-bump commit will only live on the tag; releases still work).

## Publishing to Flathub

The Flatpak manifest lives in `packaging/flatpak/` and is already built in CI. Getting it onto Flathub, so it appears in Software Manager / GNOME Software, is a one-time submission:

1. Fork [flathub/flathub](https://github.com/flathub/flathub) and create a branch named `io.github.theamanullahdev.Paperloom` from its `new-pr` branch.
2. In that branch, add:
   - `io.github.theamanullahdev.Paperloom.yml`: a copy of our manifest, with the `paperloom` module's `type: dir` source replaced by a pinned git source:
     ```yaml
     - type: git
       url: https://github.com/theamanullahdev/paperloom.git
       tag: v0.2.0
       commit: <the full commit hash of that tag>
     ```
   - `cargo-sources.json`: download it from any CI run's `flatpak-cargo-sources` artifact, or generate it with `flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json`.
3. Open a PR against `flathub/flathub`'s `new-pr` branch. Flathub's reviewers check the manifest; screenshots in the metainfo file (`packaging/flatpak/*.metainfo.xml`) are expected before approval, and none are listed yet.
4. Once merged, Flathub creates `github.com/flathub/io.github.theamanullahdev.Paperloom`, and you get write access. Each new release is then a PR there that bumps `tag`/`commit` and refreshes `cargo-sources.json`. Flathub's bot can open these automatically if you add an `x-checker-data` block to the source.

Flathub requires the app ID to match a domain you control. `io.github.theamanullahdev` is valid because this repository lives under that GitHub account.

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
