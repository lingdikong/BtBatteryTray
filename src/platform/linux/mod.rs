pub mod autostart;
pub mod bluetooth;
pub mod devwatch;
pub mod hid;

use std::time::Duration;

/// Tray icon edge length (pixels); the StatusNotifierItem host rescales it to the panel height
pub const ICON_SIZE: usize = 32;
/// Separator between device name and battery level in the menu (dbusmenu doesn't right-align on `\t`)
pub const MENU_SEPARATOR: &str = "  ";
/// Icons of disconnected devices are removed by dropping them, see `set_icon_visible`
pub const KEEP_HIDDEN_ICONS: bool = false;

/// No-op: only macOS keeps the icons of disconnected devices alive
pub fn set_icon_visible(_tray: &tray_icon::TrayIcon, _visible: bool) {}

/// An icon that exists is on the tray here, there is nothing to ask the system
pub fn icon_is_visible(_tray: &tray_icon::TrayIcon) -> bool {
    true
}

/// tray-icon's ksni backend runs D-Bus on its own thread, so no init is needed
pub fn init() {}

/// The ksni backend needs no main-thread event loop; just wait a while
pub fn pump_events(timeout: Duration) -> bool {
    std::thread::sleep(timeout);
    true
}

/// Sends a desktop notification via the freedesktop notification service (org.freedesktop.Notifications); sent asynchronously, always returns true
pub fn notify(_tray: &tray_icon::TrayIcon, _guid: u128, title: &str, body: &str) -> bool {
    let (app_name, title, body) = (rust_i18n::t!("app.name").into_owned(), title.to_owned(), body.to_owned());
    // Run on a background thread so an unresponsive notification service doesn't block the tray
    std::thread::spawn(move || {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let Ok(conn) = zbus::blocking::Connection::session() else { return };
        let hints: HashMap<&str, Value> = HashMap::from([("urgency", Value::U8(1))]);
        let _ = conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(app_name.as_str(), 0u32, "battery-caution", title.as_str(), body.as_str(), Vec::<&str>::new(), hints, -1i32),
        );
    });
    true
}

/// Reads `color-scheme` from the freedesktop settings portal: 1 prefers dark, 2 prefers light, 0 unspecified.
///
/// GNOME's top bar stays dark even in light mode, so only draw a dark foreground on an explicit light preference.
pub fn light_theme() -> bool {
    let read = || -> zbus::Result<u32> {
        let conn = zbus::blocking::Connection::session()?;
        let reply = conn.call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.freedesktop.appearance", "color-scheme"),
        )?;
        let value: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
        Ok(u32::try_from(value)?)
    };
    read().is_ok_and(|scheme| scheme == 2)
}

/// Regular apps can't get global input idle time on Wayland, so no "retry on input"
pub fn idle_time() -> Option<Duration> {
    None
}
