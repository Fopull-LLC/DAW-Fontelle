#!/usr/bin/env bash
# Builds Fontelle.app around a macOS binary (hub card 0645).
#
#   packaging/macos/make-app.sh <binary> <version> <folder>
#
# makes <folder>/Fontelle.app. Until 0.34.0 the macOS download was the bare
# binary: a double-click opened Terminal, and no macOS download ever counted
# an install. The app is signed ad hoc ("--sign -", no Apple account): not
# notarised, so the first open still needs System Settings' Open Anyway
# (FIRST-RUN.txt), but a whole, sealed app that macOS can be told to trust,
# rather than one it calls damaged.
set -euo pipefail

if [ $# -ne 3 ]; then
    echo "usage: $0 <binary> <version> <folder>" >&2
    exit 2
fi
binary=$1
version=$2
folder=$3
here=$(cd "$(dirname "$0")" && pwd)

app="$folder/Fontelle.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/fontelle"
chmod 755 "$app/Contents/MacOS/fontelle"
# CFBundleShortVersionString is three numbers by Apple's rule; the full
# version, beta and all, goes in CFBundleVersion.
short="${version%%-*}"
sed -e "s/@VERSION@/$version/g" -e "s/@SHORT_VERSION@/$short/g" \
    "$here/Info.plist.in" >"$app/Contents/Info.plist"
printf 'APPL????' >"$app/Contents/PkgInfo"

# Off a Mac (a check run elsewhere) there is no codesign; on one there
# always is, and the release runs on one.
if command -v codesign >/dev/null; then
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"
fi
if command -v plutil >/dev/null; then
    plutil -lint "$app/Contents/Info.plist"
fi
echo "make-app: $app"
