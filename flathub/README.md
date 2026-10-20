# Flathub submission files

Everything Flathub needs for `io.github.theamanullahdev.Paperloom`:

| File | What it is |
|---|---|
| `io.github.theamanullahdev.Paperloom.yml` | The manifest. Builds libass, libplacebo and mpv, then Paperloom from a pinned git tag. |
| `cargo-sources.json` | Every Rust crate Paperloom needs, listed for the offline Flathub build. Generated from the tag's `Cargo.lock`. |
| `flathub.json` | Builds for x86_64 only. |
| `update.sh` | Pins the manifest to a release tag and regenerates `cargo-sources.json`. |

The app's metadata (`packaging/flatpak/io.github.theamanullahdev.Paperloom.metainfo.xml`), desktop file and icon are read from the pinned tag, so they ship with each release.

## Submit (once)

1. Pin to the newest release: `./update.sh` (or `./update.sh v0.9.0`). Commit the result.
2. Fork [flathub/flathub](https://github.com/flathub/flathub) and create a branch from `new-pr`.
3. Copy `io.github.theamanullahdev.Paperloom.yml`, `cargo-sources.json` and `flathub.json` into the root of that branch.
4. Open a pull request against `flathub/flathub` **targeting the `new-pr` branch**, titled `Add io.github.theamanullahdev.Paperloom`.
5. A bot builds it and posts the result; reviewers may ask for changes. When it's merged, Flathub creates `github.com/flathub/io.github.theamanullahdev.Paperloom` and gives you write access.

Optional local build before submitting:

```
flatpak install flathub org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install --force-clean build io.github.theamanullahdev.Paperloom.yml
flatpak run io.github.theamanullahdev.Paperloom
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.theamanullahdev.Paperloom.yml
```

## Updating after each release

Releases are automatic on this repo. For Flathub, in the `flathub/io.github.theamanullahdev.Paperloom` repository: run `./update.sh vX.Y.Z` from this folder, copy the manifest and `cargo-sources.json` over, and open a pull request. (The `x-checker-data` block in the manifest lets Flathub's bot open these pull requests for new tags on its own.)

## Permissions, for the reviewer's questions

| Permission | Why |
|---|---|
| `--share=network` | The online library and update checks. |
| `--socket=x11`, `--socket=wayland` | The wallpaper is drawn into a desktop-level window (X11/XWayland) or a layer-shell background surface (Wayland). |
| `--device=dri` | GPU video decoding and rendering. |
| `--socket=pulseaudio` | Wallpaper audio when the user unmutes it. |
| `--filesystem=xdg-videos:ro` | Importing videos from `~/Videos` without a portal round trip. |
| `--filesystem=xdg-config/autostart:create` | The "Start Paperloom when I log in" switch writes an autostart entry. |
| `--talk-name=org.cinnamon.ScreenSaver` | Pausing the wallpaper while the screen is locked on Cinnamon. |
| `--talk-name=org.kde.StatusNotifierWatcher` | The tray icon. |
