# Downloads the Windows libmpv + ffmpeg builds Paperloom bundles, from
# shinchiro/mpv-winbuild-cmake (the builds mpv.io links to), and makes an
# MSVC import library for libmpv so `cargo build` can link it.
#
#   pwsh packaging/windows/fetch-mpv.ps1 -Out C:\mpv
#
# Afterwards: set RUSTFLAGS="-L native=C:\mpv" and build; ship
# C:\mpv\libmpv-2.dll, vulkan-1.dll and ffmpeg.exe next to the executables.
# Needs `gh` (authenticated) and an MSVC developer shell (for dumpbin/lib).
param([Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$download = Join-Path $Out "download"
New-Item -ItemType Directory -Force -Path $download | Out-Null

# The plain x86_64 builds (not "-v3", which needs AVX2).
gh release download --repo shinchiro/mpv-winbuild-cmake --pattern "mpv-dev-x86_64-2*.7z" --dir $download --clobber
gh release download --repo shinchiro/mpv-winbuild-cmake --pattern "ffmpeg-x86_64-git-*.7z" --dir $download --clobber

foreach ($archive in Get-ChildItem $download -Filter *.7z) {
    7z x $archive.FullName "-o$download\$($archive.BaseName)" -y | Out-Null
}

$dll = Get-ChildItem $download -Recurse -Filter "libmpv-2.dll" | Select-Object -First 1
$ffmpeg = Get-ChildItem $download -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
if (-not $dll) { throw "libmpv-2.dll not found in the mpv-dev archive" }
if (-not $ffmpeg) { throw "ffmpeg.exe not found in the ffmpeg archive" }
Copy-Item $dll.FullName $Out -Force
Copy-Item $ffmpeg.FullName $Out -Force

# libmpv imports vulkan-1.dll (the Khronos Vulkan loader). GPU drivers
# usually install it, but not always (VMs, basic display drivers, CI), and
# without it Windows refuses to start the engine at all. The loader is
# redistributable (Apache-2.0); LunarG publishes it as "runtime components".
$vulkanZip = Join-Path $download "vulkan-runtime.zip"
Invoke-WebRequest -Uri "https://sdk.lunarg.com/sdk/download/latest/windows/vulkan-runtime-components.zip" -OutFile $vulkanZip
Expand-Archive $vulkanZip -DestinationPath (Join-Path $download "vulkan-runtime") -Force
$candidates = Get-ChildItem (Join-Path $download "vulkan-runtime") -Recurse -Filter "vulkan-1.dll"
$candidates | ForEach-Object { Write-Host "vulkan-1.dll candidate: $($_.FullName)" }
# Prefer the x64 build; otherwise anything not explicitly 32-bit/ARM.
$vulkan = $candidates | Where-Object { $_.FullName -match '[\\/]x64[\\/]' } | Select-Object -First 1
if (-not $vulkan) { $vulkan = $candidates | Where-Object { $_.FullName -notmatch '(?i)x86|arm' } | Select-Object -First 1 }
if (-not $vulkan) { throw "vulkan-1.dll (x64) not found in the Vulkan runtime components" }
Copy-Item $vulkan.FullName $Out -Force

# mpv.lib (what `cargo:rustc-link-lib=mpv` asks the MSVC linker for), from
# the DLL's export table.
$exports = dumpbin /exports (Join-Path $Out "libmpv-2.dll") |
    ForEach-Object { if ($_ -match '^\s+\d+\s+[0-9A-F]+\s+[0-9A-F]+\s+(mpv_\w+)') { $Matches[1] } }
if (-not $exports) { throw "no mpv_* exports found in libmpv-2.dll" }
$def = Join-Path $Out "mpv.def"
@("LIBRARY libmpv-2.dll", "EXPORTS") + $exports | Set-Content -Encoding ascii $def
$lib = Join-Path $Out "mpv.lib"
lib /nologo "/def:$def" /machine:x64 "/out:$lib"
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $lib)) { throw "lib.exe failed to create $lib" }

Write-Host "libmpv ready in $Out ($($exports.Count) exports)"
