#!/bin/sh
# Build the release binary and package it as a tar.gz in target/dist/
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

cargo build --release
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
name="BtBatteryTray-$version-linux-$(uname -m)"
stage="target/dist/$name"

rm -rf "$stage"
mkdir -p "$stage"
cp target/release/bt-battery-tray "$stage/"
cp packaging/linux/bt-battery-tray.desktop packaging/linux/70-bt-battery-tray.rules packaging/linux/install.sh "$stage/"
cp assets/app.png "$stage/bt-battery-tray.png"
chmod +x "$stage/install.sh" "$stage/bt-battery-tray"
tar -C target/dist -czf "target/dist/$name.tar.gz" "$name"
echo "Package: $root/target/dist/$name.tar.gz"
