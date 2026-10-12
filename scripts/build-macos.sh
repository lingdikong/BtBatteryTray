#!/bin/sh
# Build a universal (Apple Silicon + Intel) binary and package it as .app and .dmg in target/dist/
# Requires: rustup target add aarch64-apple-darwin x86_64-apple-darwin
# Set CODESIGN_IDENTITY to sign with a Developer ID certificate; otherwise the app is ad-hoc signed
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

stage="target/dist/macos"
app="$stage/BtBatteryTray.app"
rm -rf "$stage"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/bt-battery-tray" \
    target/aarch64-apple-darwin/release/bt-battery-tray \
    target/x86_64-apple-darwin/release/bt-battery-tray
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
cp assets/app.icns "$app/Contents/Resources/AppIcon.icns"
# Localized app name (InfoPlist.strings per language)
cp -R packaging/macos/*.lproj "$app/Contents/Resources/"
codesign --force --deep --sign "${CODESIGN_IDENTITY:--}" "$app"

ln -s /Applications "$stage/Applications"
dmg="target/dist/BtBatteryTray-$version-macos.dmg"
rm -f "$dmg"
hdiutil create -volname "Bluetooth Battery" -srcfolder "$stage" -ov -format UDZO "$dmg"
echo "Package: $root/$dmg"
