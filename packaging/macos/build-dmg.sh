#!/usr/bin/env bash
# Builds Paperloom.app and a .dmg around it, bundling libmpv, ffmpeg and
# every Homebrew dylib they pull in (so the app runs without Homebrew).
#
#   brew install mpv ffmpeg dylibbundler
#   RUSTFLAGS="-L $(brew --prefix)/lib" cargo build --release --workspace
#   packaging/macos/build-dmg.sh 0.2.0 target/release out/
#
# Output: out/paperloom-<version>-macos-<arch>.dmg. The app is ad-hoc
# signed only (required to run at all on Apple Silicon); it is not
# notarized, so the first launch needs System Settings → Privacy & Security
# → "Open Anyway" (see README).
set -euo pipefail

version="$1"
bin_dir="$2"
out_dir="$3"
here="$(cd "$(dirname "$0")" && pwd)"
arch="$(uname -m)"
[ "$arch" = "arm64" ] && arch="aarch64"

work="$(mktemp -d)"
app="$work/Paperloom.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"

cp "$bin_dir/paperloom-gui" "$bin_dir/paperloom-engine" "$app/Contents/MacOS/"
cp -L "$(brew --prefix)/bin/ffmpeg" "$app/Contents/MacOS/ffmpeg"
chmod u+w "$app/Contents/MacOS/"*
cp "$here/Paperloom.icns" "$app/Contents/Resources/"

# Homebrew bottles target the macOS they were built on.
min_macos="$(sw_vers -productVersion | cut -d. -f1).0"
sed -e "s/@VERSION@/$version/g" -e "s/@MIN_MACOS@/$min_macos/g" "$here/Info.plist" > "$app/Contents/Info.plist"

# Copy every non-system dylib into Frameworks and rewrite load paths.
dylibbundler --overwrite-dir --bundle-deps --create-dir \
  --fix-file "$app/Contents/MacOS/paperloom-engine" \
  --fix-file "$app/Contents/MacOS/ffmpeg" \
  --dest-dir "$app/Contents/Frameworks" \
  --install-path "@executable_path/../Frameworks/"

# dylibbundler adds its rpath once per fixed file it touches, so a binary
# can end up with the same LC_RPATH twice — which macOS 15's dyld rejects
# outright ("duplicate LC_RPATH"). Keep exactly one of each.
dedupe_rpaths() {
  local file="$1" rpath count
  otool -l "$file" | awk '/cmd LC_RPATH/{getline; getline; print $2}' | sort | uniq -c | while read -r count rpath; do
    while [ "$count" -gt 1 ]; do
      install_name_tool -delete_rpath "$rpath" "$file"
      count=$((count - 1))
    done
  done
}
for file in "$app/Contents/MacOS/"* "$app/Contents/Frameworks/"*.dylib; do
  dedupe_rpaths "$file"
done

# Rewriting load commands invalidates signatures; re-sign everything ad hoc.
find "$app/Contents/Frameworks" -name '*.dylib' -exec codesign --force --sign - {} \;
for exe in "$app/Contents/MacOS/"*; do codesign --force --sign - "$exe"; done
codesign --force --sign - "$app"

# Smoke test: the bundled engine must load its libraries without Homebrew,
# and both bundled executables must actually start (a dyld failure aborts
# before main; a healthy engine rejects the bogus path with exit 1).
otool -L "$app/Contents/MacOS/paperloom-engine" | grep -q "@executable_path/../Frameworks/libmpv" \
  || { echo "libmpv wasn't rewritten into the bundle" >&2; exit 1; }
set +e
"$app/Contents/MacOS/paperloom-engine" --preview /does-not-exist.mp4
engine_code=$?
"$app/Contents/MacOS/ffmpeg" -version > /dev/null
ffmpeg_code=$?
set -e
[ "$engine_code" = 1 ] || { echo "bundled paperloom-engine failed to start (exit $engine_code)" >&2; exit 1; }
[ "$ffmpeg_code" = 0 ] || { echo "bundled ffmpeg failed to start (exit $ffmpeg_code)" >&2; exit 1; }

mkdir -p "$out_dir" "$work/dmg"
cp -R "$app" "$work/dmg/"
ln -s /Applications "$work/dmg/Applications"
dmg="$out_dir/paperloom-$version-macos-$arch.dmg"
hdiutil create -volname "Paperloom $version" -srcfolder "$work/dmg" -ov -format UDZO "$dmg"
echo "built $dmg"
