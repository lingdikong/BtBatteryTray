pub mod autostart;
pub mod bluetooth;
pub mod devwatch;
pub mod hid;
mod powersource;
mod registry;

use std::time::Duration;

use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEventMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSString, NSUserDefaults};

/// Tray icon edge length (pixels): menu bar icons are at most 22pt, drawn for 2x displays
pub const ICON_SIZE: usize = 44;
/// Separator between device name and battery level in the menu
pub const MENU_SEPARATOR: &str = "  ";
/// Keep the tray icon of a disconnected device alive and hidden, see `set_icon_visible`
pub const KEEP_HIDDEN_ICONS: bool = true;

/// Shows / hides a tray icon by toggling the underlying `NSStatusItem`, the way AppKit expects a menu bar item
/// to come and go: the item keeps its slot and its saved position, and comes back where it was.
///
/// Dropping the `TrayIcon` instead (what tray-icon's own `set_visible(false)` does on macOS) calls
/// `removeStatusItem`, and the item created when the device reconnects is a new one: it loses the position and is
/// sometimes never drawn at all, leaving the device without an icon until the app is restarted.
///
/// Called on every refresh for the icons that should be visible, so an item AppKit left hidden repairs itself.
pub fn set_icon_visible(tray: &tray_icon::TrayIcon, visible: bool) {
    if let Some(item) = tray.ns_status_item()
        && item.isVisible() != visible
    {
        item.setVisible(visible);
    }
}

/// Whether AppKit is currently showing the icon. Asked instead of trusting our own bookkeeping, so that an item
/// the system hid behind our back is noticed and shown again on the next refresh.
pub fn icon_is_visible(tray: &tray_icon::TrayIcon) -> bool {
    tray.ns_status_item().is_some_and(|item| item.isVisible())
}

/// Initializes NSApplication: shown only in the menu bar, not in the Dock
pub fn init() {
    let mtm = MainThreadMarker::new().expect("tray must be created on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.finishLaunching();
}

/// Processes AppKit events on the main thread for at most `timeout`
pub fn pump_events(timeout: Duration) -> bool {
    let mtm = MainThreadMarker::new().expect("event loop must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    autoreleasepool(|_| {
        let until = NSDate::dateWithTimeIntervalSinceNow(timeout.as_secs_f64());
        // SAFETY: called on the main thread, all arguments are valid objects
        while let Some(event) = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&until), NSDefaultRunLoopMode, true)
        } {
            app.sendEvent(&event);
        }
    });
    true
}

/// Sends a system notification via AppleScript: needs no .app bundle or notification permission, but it shows as coming from "Script Editor".
/// Sent asynchronously, always returns true
pub fn notify(_tray: &tray_icon::TrayIcon, _guid: u128, title: &str, body: &str) -> bool {
    // AppleScript string literal: escape backslashes and double quotes
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!("display notification {} with title {} sound name \"default\"", quote(body), quote(title));
    // Wait for osascript on a background thread to avoid leaving a zombie process
    std::thread::spawn(move || {
        let _ = std::process::Command::new("/usr/bin/osascript").args(["-e", &script]).status();
    });
    true
}

/// Whether the system appearance is light (AppleInterfaceStyle is "Dark" only in dark mode)
pub fn light_theme() -> bool {
    let style = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str("AppleInterfaceStyle"));
    !style.is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"))
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

/// Time since the last keyboard/mouse input (doesn't need the "Input Monitoring" permission)
pub fn idle_time() -> Option<Duration> {
    const COMBINED_SESSION_STATE: i32 = 0;
    const ANY_INPUT_EVENT: u32 = u32::MAX;
    // SAFETY: pure query function
    let secs = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) };
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}
