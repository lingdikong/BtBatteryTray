# bt-battery-tray

English | [简体中文](README.zh-CN.md)

A tray / menu bar app that shows the battery level of every connected Bluetooth device, on Windows, macOS and Linux. Each connected device gets its own tray icon, styled after the iPhone battery widget: the outer ring shows the battery level, and the center shows a lightning bolt while charging or the device type otherwise (keyboard / mouse / headphones / gamepad / phone / other). A dashed ring means the level could not be read. Hover for the device name and percentage.

No vendor drivers or background services needed; it is a single executable.

## Battery sources

| Platform | System battery level | Connection events |
|---|---|---|
| Windows | Device property `DEVPKEY_Bluetooth_Battery` (BLE Battery Service, HFP headsets); dual-mode devices such as iPhones use the BLE node with the same ContainerId | WinRT DeviceWatcher |
| Linux | BlueZ `org.bluez.Battery1`; otherwise the kernel's `/sys/class/power_supply` (Logitech HID++ etc., which also reports charging) | BlueZ D-Bus signals |
| macOS | `system_profiler SPBluetoothDataType` (Apple devices, AirPods, most BLE devices) | IOHIDManager (Bluetooth HID devices only; AirPods rely on periodic refresh) |

Keyboards built on the RongYuan platform (VID `0x3151`, e.g. Feker NX87, Akko, MonsGeek, Epomaker...) don't report their battery to the OS. The app reads it over the vendor HID channel with the read-only command `GET_BATTERY (0x83)`, on all three platforms. The vendor protocol is only tried for devices the OS has no battery level for.

### RongYuan protocol

Vendor collection: UsagePage `0xFF66` (Classic) / `0xFF55` (BLE), Usage `0x0202`, Report ID 6, 66-byte reports.

```text
Request:  06 55 83 00 00 00 00 00 00 7C ...    last meaningful byte is the checksum 0xFF - sum(cmd..+6)
Response: 06 55 83 28 01 ...                   0x28 = 40%, 01 = charging (0 not charging / 2 full)
```

References: [monsgeek-akko-linux](https://github.com/echtzeit-solutions/monsgeek-akko-linux), [rongyuan-kb-software](https://github.com/RieGan/rongyuan-kb-software).

## Usage

```sh
target/release/bt-battery-tray                # start the tray app (see Building below)
target/release/bt-battery-tray --print        # print battery levels once and exit (diagnostics; on Windows redirect the output, e.g. `| cat`)
target/release/bt-battery-tray --watch        # print Bluetooth connection events for 30 seconds (diagnostics)
target/release/bt-battery-tray --notify-test  # show a sample low-battery notification to check notifications aren't blocked
```

Left-click an icon to refresh; the right-click menu has the device list, Refresh now, Launch at login and Quit. Levels refresh as soon as a Bluetooth device connects or disconnects, and every 10 minutes otherwise; unreadable devices are retried with a backoff starting at 1 second, and on Windows and macOS a sleeping keyboard is retried as soon as there is keyboard or mouse input. Icon colors follow the light/dark taskbar or menu bar.

**Low-battery alert:** when a device drops to 20% or below and isn't charging, its ring turns red and a system notification is shown once. It alerts again only after the device has charged or risen above 25%. Windows uses a tray balloon (shown as a regular notification on Windows 10/11), Linux the freedesktop notification service, and macOS `osascript` (the notification appears to come from Script Editor).

**Launch at login:** Windows writes `HKCU\...\Run`, macOS `~/Library/LaunchAgents/com.lingdikong.btbatterytray.plist`, Linux `~/.config/autostart/bt-battery-tray.desktop`.

### Language

The UI follows the system language: English and Simplified Chinese are included, and anything else falls back to English. Set `BT_BATTERY_LANG` (e.g. `BT_BATTERY_LANG=en`) to override it.

Translations live in `locales/*.yml`, with `en.yml` as the source language. To add a language, copy `en.yml` to e.g. `locales/ja.yml` and translate the values; `cargo test` checks that every locale has exactly the keys and `%{placeholders}` of `en.yml`. Installer and package names are localized separately: `installer/setup.iss` (`[CustomMessages]`), `packaging/macos/*.lproj/InfoPlist.strings` and `packaging/linux/bt-battery-tray.desktop`.

### Platform notes

- **Windows**: new tray icons start in the overflow area; drag one to the taskbar once and it is remembered per device.
- **macOS**: reading RongYuan keyboards requires allowing the app under System Settings > Privacy & Security > Input Monitoring. Without a Developer ID signature, open it the first time with right-click > Open. iPhone battery levels are not exposed by the system.
- **Linux**: the tray uses StatusNotifierItem (supported natively by KDE, Xfce and others; GNOME needs an AppIndicator extension). RongYuan keyboards need the udev rule `packaging/linux/70-bt-battery-tray.rules` (`install.sh --udev`). Wayland offers no global idle time, so there is no retry-on-input.

## Code layout

- `src/main.rs`: tray UI, refresh thread, merging battery sources (cross-platform)
- `src/icon.rs`: icon drawing; `src/rongyuan.rs`: RongYuan protocol; `src/bluetooth.rs`: shared device types; `src/i18n.rs`: language selection
- `src/platform/{windows,linux,macos}/`: per-platform Bluetooth enumeration (`bluetooth`), connection events (`devwatch`), HID I/O (`hid`), launch at login (`autostart`), event loop and notifications
- `locales/`: UI translations

## Building

### Prerequisites

| Platform | Requirements |
|---|---|
| All | Rust 1.88 or newer (stable), installed with [rustup](https://rustup.rs) |
| Windows | Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC linker and Windows SDK); the rustup installer offers to install it |
| macOS | Xcode Command Line Tools: `xcode-select --install` |
| Linux | A C toolchain for linking (`build-essential` on Debian/Ubuntu, `base-devel` on Arch); no GTK or other system libraries are needed. At runtime: BlueZ and a D-Bus session |

### Build and run

```sh
git clone https://github.com/lingdikong/BtBatteryTray.git
cd BtBatteryTray
cargo build --release          # binary: target/release/bt-battery-tray (.exe on Windows)
cargo run                      # debug build, starts the tray app
cargo run -- --print           # debug build, prints battery levels once
BT_BATTERY_LANG=zh-CN cargo run   # force the UI language (PowerShell: $env:BT_BATTERY_LANG="zh-CN"; cargo run)
```

### Tests and checks

```sh
cargo test                                  # unit tests, including the locale key / placeholder check
cargo clippy --all-targets -- -D warnings   # lints, same as CI
```

The platform code can be type-checked for the other operating systems from a single machine (compile only, no linking):

```sh
rustup target add x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin
cargo clippy --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
cargo clippy --target aarch64-apple-darwin --all-targets -- -D warnings
```

### Regenerating the app icons

`assets/app.ico`, `app.icns` and `app.png` are drawn by `src/icon.rs` and committed. After changing the drawing code, regenerate them (needs Python 3, no extra packages):

```sh
APP_ICON_DIR=/tmp/appicon cargo test app_icon        # PowerShell: $env:APP_ICON_DIR="$env:TEMP\appicon"; cargo test app_icon
python scripts/make_ico.py /tmp/appicon assets/app.ico assets/app.icns assets/app.png
```

## Packaging

The version number comes from `version` in `Cargo.toml`; bump it before building a release. Each package must be built on its own platform.

### Windows installer

1. Install [Inno Setup 6.5+](https://jrsoftware.org/isinfo.php): `winget install JRSoftware.InnoSetup`
2. Build: `powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1` (or just `scripts\build-installer.ps1` from PowerShell)
3. Output: `target\installer\BtBatteryTray-Setup-<version>.exe`

The script builds the release exe and compiles `installer/setup.iss`. The installer installs per user by default (no admin rights), optionally with launch at login and a desktop shortcut, and shows English or Simplified Chinese depending on the Windows UI language. Upgrading or uninstalling stops the running tray app first, and uninstalling removes the autostart entry.

### macOS (.dmg)

1. Add both architectures: `rustup target add aarch64-apple-darwin x86_64-apple-darwin`
2. Build: `sh scripts/build-macos.sh`
3. Output: `target/dist/BtBatteryTray-<version>-macos.dmg`, containing a universal (Apple Silicon + Intel) `BtBatteryTray.app` and an Applications shortcut

Signing: by default the app is ad-hoc signed, so users have to open it the first time with right-click > Open. With an Apple Developer account, sign with your certificate and then notarize:

```sh
CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" sh scripts/build-macos.sh
xcrun notarytool submit target/dist/BtBatteryTray-<version>-macos.dmg --keychain-profile <profile> --wait
xcrun stapler staple target/dist/BtBatteryTray-<version>-macos.dmg
```

### Linux (.tar.gz)

1. Build: `sh scripts/build-linux.sh` (builds for the current machine's architecture)
2. Output: `target/dist/BtBatteryTray-<version>-linux-<arch>.tar.gz`, containing the binary, icon, `.desktop` file, udev rule and `install.sh`
3. Install on the target machine:

   ```sh
   tar xzf BtBatteryTray-<version>-linux-x86_64.tar.gz
   cd BtBatteryTray-<version>-linux-x86_64
   ./install.sh            # installs into ~/.local; set PREFIX=... to change
   ./install.sh --udev     # additionally installs the udev rule with sudo (RongYuan keyboards)
   ```

   To uninstall, delete `~/.local/bin/bt-battery-tray`, `~/.local/share/applications/bt-battery-tray.desktop`, `~/.local/share/icons/hicolor/256x256/apps/bt-battery-tray.png` and, if installed, `/etc/udev/rules.d/70-bt-battery-tray.rules`.

### CI builds and releases

`.github/workflows/build.yml` runs on pushes to `main`, on `v*` tags, on pull requests and manually. On Windows, macOS and Linux it runs clippy and the tests, then builds the packages above.

**Downloading a build:** open the repository's **Actions** tab, click a successful run, and download `BtBatteryTray-Windows` / `-macOS` / `-Linux` from the **Artifacts** section at the bottom of the page (sign-in required; each is a zip containing the package). Artifacts expire after 90 days, so use Releases for anything you hand out.

**Publishing a release:** pushing a `v*` tag also publishes a GitHub Release with the packages of all three platforms and release notes generated from the commits:

```sh
# 1. Bump version in Cargo.toml (e.g. 0.1.6) and commit
git commit -am "release: v0.1.6"
# 2. Tag and push
git tag v0.1.6
git push origin main v0.1.6
```

The tag must match `version` in `Cargo.toml` (`v0.1.6` ↔ `0.1.6`); otherwise the run fails before building, so the release can't ship packages with the wrong version. If a release run fails, fix the problem, delete the tag (`git tag -d v0.1.6 && git push origin :refs/tags/v0.1.6`) and tag again.

### Packaging files

- `installer/setup.iss`: Windows installer script; `installer/ChineseSimplified.isl`: Simplified Chinese messages (from the official Inno Setup repository)
- `packaging/macos/`: `Info.plist` template and localized app names (`*.lproj/InfoPlist.strings`)
- `packaging/linux/`: `.desktop` file, udev rule and `install.sh`
- `scripts/`: `build-installer.ps1`, `build-macos.sh`, `build-linux.sh`, `make_ico.py`
