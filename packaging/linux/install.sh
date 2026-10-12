#!/bin/sh
# Install into the user's directory (~/.local by default); no root needed.
# With --udev, also install the udev rule via sudo (needed to read RongYuan keyboard batteries).
set -eu
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"

install -Dm755 bt-battery-tray "$PREFIX/bin/bt-battery-tray"
install -Dm644 bt-battery-tray.png "$PREFIX/share/icons/hicolor/256x256/apps/bt-battery-tray.png"
mkdir -p "$PREFIX/share/applications"
sed "s|^Exec=.*|Exec=\"$PREFIX/bin/bt-battery-tray\"|" bt-battery-tray.desktop \
    > "$PREFIX/share/applications/bt-battery-tray.desktop"
echo "Installed to $PREFIX/bin/bt-battery-tray; start \"Bluetooth Battery\" from the application menu."

UDEV_CMD="sudo install -Dm644 70-bt-battery-tray.rules /etc/udev/rules.d/70-bt-battery-tray.rules && sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw"
if [ "${1:-}" = "--udev" ]; then
    sh -c "$UDEV_CMD"
    echo "udev rule installed."
else
    echo "RongYuan keyboards (Feker, Akko, MonsGeek, ...) need a udev rule to report their battery. Re-run ./install.sh --udev, or run:"
    echo "  $UDEV_CMD"
fi
echo "GNOME hides tray icons by default; install an AppIndicator extension (e.g. Ubuntu's ubuntu-appindicators)."
