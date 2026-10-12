//! Platform-specific parts. Each platform module provides the same set of interfaces:
//!
//! - `bluetooth::connected_devices()`: connected Bluetooth devices and the battery level reported by the OS
//! - `devwatch::ConnectionWatcher::start(tx, msg)`: sends `msg` to `tx` when a Bluetooth device connects / disconnects
//! - `hid::vendor_interfaces()`: RongYuan vendor HID channels (`rongyuan` uses them to send/receive reports)
//! - `autostart::{is_enabled, set_enabled}`: launch at startup / login
//! - `init()`, `pump_events()`: the event loop the tray needs
//! - `light_theme()`: whether the taskbar / menu bar uses a light theme
//! - `idle_time()`: time since the last keyboard/mouse input (None if unavailable)
//! - `notify(tray, guid, title, body)`: shows a system notification (low battery alert), returns whether it was sent
//! - `ICON_SIZE`, `MENU_SEPARATOR`

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;
