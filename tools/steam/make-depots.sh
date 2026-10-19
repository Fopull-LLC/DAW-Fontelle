#!/usr/bin/env bash
# Lays a published release out as Steam depots (hub card 0429; docs/steam.md).
#
#   tools/steam/make-depots.sh <tag> [out-dir]
#
# Reads only: the release's archives come down with `gh release download`
# and are checked against its SHA256SUMS before anything is unpacked. The
# bytes in the depots are the bytes everybody else downloaded — nothing is
# rebuilt, so a Steam build is exactly a release CI already passed.
#
# Out (default `steam-<version>/`):
#
#   depots/linux/       fontelle, licences, the desktop entry and icon
#   depots/windows/     fontelle.exe, fontelle-asio.exe, licences
#   depots/macos/       Fontelle.app (one universal binary), licences
#   scripts/            app_build.vdf + one depot_build_*.vdf per depot
#   downloads/          the archives and SHA256SUMS, as published
#
# Each depot carries `fontelle-steam.txt` beside the program (on macOS in
# the bundle's `Contents/Resources`): that file is how the program knows
# Steam installed it, and Steam updates it — the start menu's updater is off
# (`fontelle_app::distribution`). No DRM wrapper is applied; there is no
# steam_api either. Uploading is the one step left:
#
#   steamcmd +login <builder> +run_app_build "$PWD/<out>/scripts/app_build.vdf" +quit
#
# The ids come from the environment, or stay as FILL_ME placeholders that
# steamcmd refuses: STEAM_APP_ID, STEAM_DEPOT_LINUX, STEAM_DEPOT_WINDOWS,
# STEAM_DEPOT_MACOS. Ty's, from Steamworks.
#
# Needs gh, tar, unzip, sha256sum (or shasum), and for the macOS depot lipo
# (macOS) or llvm-lipo (anywhere LLVM is): the release has an Apple silicon
# and an Intel binary, and Steam's macOS depot is one app for both.
set -euo pipefail

tag="${1:?usage: make-depots.sh <tag> [out-dir]}"
version="${tag#v}"
out="${2:-steam-$version}"
repo="${GITHUB_REPOSITORY:-Fopull-LLC/DAW-Fontelle}"
here=$(cd "$(dirname "$0")" && pwd)
tree=$(cd "$here/../.." && pwd)

app_id="${STEAM_APP_ID:-FILL_ME_APP_ID}"
depot_linux="${STEAM_DEPOT_LINUX:-FILL_ME_DEPOT_LINUX}"
depot_windows="${STEAM_DEPOT_WINDOWS:-FILL_ME_DEPOT_WINDOWS}"
depot_macos="${STEAM_DEPOT_MACOS:-FILL_ME_DEPOT_MACOS}"

linux="fontelle-$version-x86_64-unknown-linux-gnu"
windows="fontelle-$version-x86_64-pc-windows-msvc"
mac_arm="fontelle-$version-aarch64-apple-darwin"
mac_intel="fontelle-$version-x86_64-apple-darwin"

die() {
    echo "make-depots: $*" >&2
    exit 1
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

lipo_tool() {
    if command -v lipo >/dev/null 2>&1; then
        echo lipo
    elif command -v llvm-lipo >/dev/null 2>&1; then
        echo llvm-lipo
    else
        die "no lipo or llvm-lipo to make the macOS app universal (install LLVM, or run on macOS)"
    fi
}

for tool in gh tar unzip; do
    command -v "$tool" >/dev/null 2>&1 || die "needs $tool"
done
lipo=$(lipo_tool)

[ -e "$out/depots" ] && die "$out/depots already exists — remove it or name another folder"
mkdir -p "$out/downloads" "$out/depots/linux" "$out/depots/windows" "$out/depots/macos" \
    "$out/scripts" "$out/unpack"
out=$(cd "$out" && pwd)

echo "make-depots: downloading $tag from $repo"
gh release download "$tag" --repo "$repo" --dir "$out/downloads" --clobber \
    --pattern "$linux.tar.gz" --pattern "$windows.zip" \
    --pattern "$mac_arm.tar.gz" --pattern "$mac_intel.tar.gz" \
    --pattern SHA256SUMS

# Every archive against the checksum file the release workflow wrote. An
# archive the file does not list is as bad as one that does not match.
for archive in "$linux.tar.gz" "$windows.zip" "$mac_arm.tar.gz" "$mac_intel.tar.gz"; do
    want=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$out/downloads/SHA256SUMS")
    [ -n "$want" ] || die "SHA256SUMS does not list $archive"
    have=$(sha256_of "$out/downloads/$archive")
    [ "$want" = "$have" ] || die "$archive does not match SHA256SUMS ($have, expected $want)"
    echo "make-depots: verified $archive"
done

tar -xzf "$out/downloads/$linux.tar.gz" -C "$out/unpack"
tar -xzf "$out/downloads/$mac_arm.tar.gz" -C "$out/unpack"
tar -xzf "$out/downloads/$mac_intel.tar.gz" -C "$out/unpack"
unzip -q "$out/downloads/$windows.zip" -d "$out/unpack"

marker() {
    cat >"$1/fontelle-steam.txt" <<'EOF'
This copy of Fontelle was installed by Steam, and Steam keeps it up to date:
its own updater is switched off. Fontelle keeps your settings, soundfonts,
presets and logs in your user folders, never here.
EOF
}

# Linux and Windows: the release folder's contents at the depot's top, as
# they are in the archive. Linux's install.sh is for a hand install from the
# tarball (a menu entry pointing at ~/.local/bin); Steam makes its own.
cp -R "$out/unpack/$linux/." "$out/depots/linux/"
rm -f "$out/depots/linux/install.sh"
marker "$out/depots/linux"
cp -R "$out/unpack/$windows/." "$out/depots/windows/"
marker "$out/depots/windows"

# macOS: the release has a bare binary per architecture and no .app; Steam
# launches an app bundle. One universal binary in a minimal bundle, the
# licences beside it at the depot's top as in the other two.
app="$out/depots/macos/Fontelle.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
"$lipo" -create -output "$app/Contents/MacOS/fontelle" \
    "$out/unpack/$mac_arm/fontelle" "$out/unpack/$mac_intel/fontelle"
chmod 755 "$app/Contents/MacOS/fontelle"
# CFBundleShortVersionString is three numbers by Apple's rule; the full
# version, beta and all, goes in CFBundleVersion.
short="${version%%-*}"
sed -e "s/@VERSION@/$version/g" -e "s/@SHORT_VERSION@/$short/g" \
    "$tree/packaging/macos/Info.plist.in" >"$app/Contents/Info.plist"
marker "$app/Contents/Resources"
for item in "$out/unpack/$mac_arm/"*; do
    [ "$(basename "$item")" = fontelle ] || cp -R "$item" "$out/depots/macos/"
done

for template in "$here"/*.vdf.in; do
    name=$(basename "$template" .in)
    sed -e "s/@APP_ID@/$app_id/g" -e "s/@VERSION@/$version/g" \
        -e "s/@DEPOT_LINUX@/$depot_linux/g" -e "s/@DEPOT_WINDOWS@/$depot_windows/g" \
        -e "s/@DEPOT_MACOS@/$depot_macos/g" "$template" >"$out/scripts/$name"
done

rm -rf "$out/unpack"
echo "make-depots: $out"
du -sh "$out/depots/linux" "$out/depots/windows" "$out/depots/macos"
if grep -q FILL_ME "$out/scripts/"*.vdf; then
    echo "make-depots: the app and depot ids are placeholders — set STEAM_APP_ID and STEAM_DEPOT_* (docs/steam.md)"
fi
