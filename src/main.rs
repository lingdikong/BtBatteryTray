#![windows_subsystem = "windows"]

mod bluetooth;
mod i18n;
mod icon;
mod platform;
mod rongyuan;

use bluetooth::DeviceKind;
use rust_i18n::t;
use platform::{autostart, devwatch};

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

rust_i18n::i18n!("locales", fallback = "en");

const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
const RETRY_DELAYS: [Duration; 9] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(3),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
];
/// Right after a device connects the system is still loading drivers / battery properties, so wait before reading
const SETTLE_DELAY: Duration = Duration::from_millis(1500);
/// Minimum interval between retries of grayed-out devices on user input
const INPUT_RETRY_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone, Copy)]
enum Trigger {
    /// User clicked refresh
    Manual,
    /// A Bluetooth device connected or disconnected
    DeviceChanged,
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    /// Bluetooth address, used as the tray icon's identity
    key: String,
    name: String,
    kind: DeviceKind,
    level: Option<u8>,
    charging: bool,
    /// Has a RongYuan private channel: a missing battery level usually means the keyboard is asleep / not ready, worth retrying
    has_vendor_channel: bool,
}

impl Entry {
    fn status(&self) -> String {
        match self.level {
            Some(l) if self.charging => t!("status.charging", level = l).into_owned(),
            Some(l) => format!("{l}%"),
            None => t!("status.unknown").into_owned(),
        }
    }
}

/// Merge battery levels from the system Bluetooth stack and from vendor private protocols
fn collect() -> Vec<Entry> {
    let devices = platform::bluetooth::connected_devices();
    // Only touch vendor channels when some device has no battery level from the system
    let vendor = if devices.iter().any(|d| d.battery.is_none()) {
        rongyuan::query_all()
    } else {
        Vec::new()
    };

    let mut used = vec![false; vendor.len()];
    let mut entries: Vec<Entry> = devices
        .into_iter()
        .map(|d| {
            // Match by Bluetooth address first; fall back to matching by name where no address is available (e.g. macOS)
            let matched = || {
                vendor.iter().position(|v| v.matches_address(&d.address)).or_else(|| {
                    vendor.iter().zip(&used).position(|(v, u)| !*u && v.product.eq_ignore_ascii_case(&d.name))
                })
            };
            if d.battery.is_none()
                && let Some(i) = matched()
            {
                used[i] = true;
                return Entry {
                    key: d.address,
                    name: d.name,
                    kind: d.kind,
                    level: vendor[i].level,
                    charging: vendor[i].charging,
                    has_vendor_channel: true,
                };
            }
            Entry {
                key: d.address,
                name: d.name,
                kind: d.kind,
                level: d.battery,
                charging: d.charging,
                has_vendor_channel: false,
            }
        })
        .collect();
    // Shouldn't happen in theory: a vendor channel is found but has no matching device in the system Bluetooth list
    for (v, _) in vendor.iter().zip(&used).filter(|(v, u)| !**u && v.level.is_some()) {
        // RongYuan devices are almost always keyboards
        entries.push(Entry {
            key: v.product.clone(),
            name: v.product.clone(),
            kind: DeviceKind::Keyboard,
            level: v.level,
            charging: v.charging,
            has_vendor_channel: true,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn spawn_worker(results: Sender<Vec<Entry>>, trigger_rx: Receiver<Trigger>) {
    thread::spawn(move || {
        let mut retry = 0;
        loop {
            let entries = collect();
            // If a connected device has no battery level (Bluetooth not ready at boot, keyboard just woke up, etc.), retry soon with backoff
            let wait = if entries.iter().any(|e| e.level.is_none()) && retry < RETRY_DELAYS.len() {
                retry += 1;
                RETRY_DELAYS[retry - 1]
            } else {
                if entries.iter().all(|e| e.level.is_some()) {
                    retry = 0;
                }
                REFRESH_INTERVAL
            };
            if results.send(entries).is_err() {
                return;
            }
            match trigger_rx.recv_timeout(wait) {
                Ok(trigger) => {
                    // New event: restart the fast retries
                    retry = 0;
                    if matches!(trigger, Trigger::DeviceChanged) {
                        thread::sleep(SETTLE_DELAY);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // Coalesce back-to-back refresh requests
            while trigger_rx.try_recv().is_ok() {}
        }
    });
}

/// A device's tray icon. On macOS it outlives the device's connection: when the device goes away the
/// `NSStatusItem` is hidden rather than destroyed, so it keeps its place in the menu bar (see
/// `platform::set_icon_visible`). Elsewhere a hidden icon is dropped right away, which removes it.
struct DeviceIcon {
    /// Entry::key of the device the icon belongs to
    key: String,
    tray: TrayIcon,
    /// Whether the icon is currently on the tray
    shown: bool,
}

struct App {
    menu: Menu,
    device_items: Vec<MenuItem>,
    refresh: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
    /// One tray icon per connected device (shown in "disconnected" style when no battery level)
    icons: Vec<DeviceIcon>,
    /// Placeholder icon when no device is connected, so the menu can still be opened
    placeholder: Option<TrayIcon>,
    entries: Option<Vec<Entry>>,
    light_theme: bool,
    /// Devices whose battery level has been read during this run
    seen_battery: std::collections::HashSet<String>,
    /// Devices already shown a low-battery reminder that haven't been charged since
    low_notified: std::collections::HashSet<String>,
}

/// Key used by the placeholder icon in icon_guid / the tray id
const PLACEHOLDER_KEY: &str = "placeholder";

/// Stable identity of a tray icon (only Windows uses the GUID; other platforms ignore it).
///
/// Without a GUID, Windows identifies icons by "exe path + creation index within the process", and the index changes with device connection order,
/// so every new icon is treated as never seen before, hidden in the overflow area by default, and the user's "show on taskbar" setting isn't remembered.
/// Deriving a fixed GUID from the device address makes the setting follow the device.
/// For unsigned programs the system binds the GUID to the exe path; reusing it from another path fails to register (silently),
/// so the exe path is hashed in too: different paths (debug build / installed build) get their own GUIDs.
fn icon_guid(key: &str) -> u128 {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    // FNV-1a with two different seeds concatenated into 128 bits; not DefaultHasher, whose output isn't guaranteed stable across Rust versions
    let fnv = |seed: u64| {
        exe.bytes().chain([0]).chain(key.bytes()).fold(seed, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
    };
    let raw = (fnv(0xcbf2_9ce4_8422_2325) as u128) << 64 | fnv(0x6c62_272e_07bb_0142) as u128;
    // Mark as an RFC 4122 v8 (custom) UUID
    raw & !(0xf << 76) & !(0x3 << 62) | 0x8 << 76 | 0x2 << 62
}

impl App {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let refresh = MenuItem::new(t!("menu.refresh"), true, None);
        let autostart = CheckMenuItem::new(t!("menu.autostart"), true, autostart::is_enabled(), None);
        let quit = MenuItem::new(t!("menu.quit"), true, None);
        let loading = MenuItem::new(t!("menu.loading"), false, None);
        menu.append_items(&[
            &loading,
            &PredefinedMenuItem::separator(),
            &refresh,
            &autostart,
            &PredefinedMenuItem::separator(),
            &quit,
        ])?;
        let mut app = Self {
            menu,
            device_items: vec![loading],
            refresh,
            autostart,
            quit,
            icons: Vec::new(),
            placeholder: None,
            entries: None,
            light_theme: platform::light_theme(),
            seen_battery: Default::default(),
            low_notified: Default::default(),
        };
        app.placeholder = Some(app.build_icon(PLACEHOLDER_KEY, None, false, DeviceKind::Other, &t!("tray.loading"))?);
        Ok(app)
    }

    fn render(&self, level: Option<u8>, charging: bool, kind: DeviceKind) -> tray_icon::Icon {
        icon::render(platform::ICON_SIZE, level, charging, kind, icon::Style { light_theme: self.light_theme })
    }

    fn build_icon(
        &self,
        key: &str,
        level: Option<u8>,
        charging: bool,
        kind: DeviceKind,
        tip: &str,
    ) -> tray_icon::Result<TrayIcon> {
        TrayIconBuilder::new()
            // Linux StatusNotifierItem remembers the icon's show / hide setting by id
            .with_id(format!("bt-battery-{key}"))
            .with_guid(icon_guid(key))
            // macOS saves where the user ⌘-dragged an icon under this name; without one AppKit numbers the icons
            // by creation order (Item-0, Item-1...), which isn't stable here since icons come and go with the
            // devices, so two icons could come back holding each other's positions
            .with_autosave_name(format!("com.lingdikong.btbatterytray.{key}"))
            .with_icon(self.render(level, charging, kind))
            .with_tooltip(tip)
            .with_menu(Box::new(self.menu.clone()))
            // Left click refreshes; the menu only opens on right click
            .with_menu_on_left_click(false)
            .build()
    }

    fn update(&mut self, entries: Vec<Entry>) {
        // Redraw after a theme change even if the data hasn't changed
        let light_theme = platform::light_theme();
        // If creating or showing an icon failed last time, retry even if the data hasn't changed.
        // The system is asked rather than our own `shown`, so an icon it stopped showing is rebuilt instead of
        // staying missing until the app is restarted
        let all_icons_shown = entries
            .iter()
            .all(|e| self.icons.iter().any(|i| i.key == e.key && i.shown && platform::icon_is_visible(&i.tray)));
        if self.entries.as_ref() == Some(&entries) && self.light_theme == light_theme && all_icons_shown {
            return;
        }
        self.light_theme = light_theme;
        self.update_menu(&entries);
        self.seen_battery.extend(entries.iter().filter(|e| e.level.is_some()).map(|e| e.key.clone()));

        // Take the icons of disconnected devices off the tray
        for icon in self.icons.iter_mut().filter(|i| !entries.iter().any(|e| e.key == i.key)) {
            icon.shown = false;
            platform::set_icon_visible(&icon.tray, false);
        }
        if !platform::KEEP_HIDDEN_ICONS {
            // Dropping a TrayIcon removes it from the tray
            self.icons.retain(|i| i.shown);
        }
        for e in &entries {
            // Tray tooltips are limited to 127 characters
            let tip: String = format!("{}\n{}", e.name, e.status()).chars().take(127).collect();
            let image = self.render(e.level, e.charging, e.kind);
            match self.icons.iter().position(|i| i.key == e.key) {
                Some(i) => {
                    let icon = &mut self.icons[i];
                    let _ = icon.tray.set_icon(Some(image));
                    let _ = icon.tray.set_tooltip(Some(tip));
                    icon.shown = true;
                    // Reassert it on every refresh: an icon AppKit stopped drawing comes back on its own
                    // instead of staying missing until the app is restarted
                    platform::set_icon_visible(&icon.tray, true);
                }
                None => {
                    if let Ok(tray) = self.build_icon(&e.key, e.level, e.charging, e.kind, &tip) {
                        self.icons.push(DeviceIcon { key: e.key.clone(), tray, shown: true });
                    }
                }
            }
        }

        if self.icons.iter().all(|i| !i.shown) {
            let tip = &*t!("tray.no_devices");
            match &self.placeholder {
                Some(tray) => {
                    let _ = tray.set_icon(Some(self.render(None, false, DeviceKind::Other)));
                    let _ = tray.set_tooltip(Some(tip));
                }
                None => self.placeholder = self.build_icon(PLACEHOLDER_KEY, None, false, DeviceKind::Other, tip).ok(),
            }
        } else {
            self.placeholder = None;
        }
        self.notify_low_battery(&entries);
        self.entries = Some(entries);
    }

    /// Remind once per device when the battery level drops to LOW_BATTERY or below (and not charging);
    /// re-arm only after charging or rising more than 5 points above the threshold, to avoid repeated reminders when hovering near it
    fn notify_low_battery(&mut self, entries: &[Entry]) {
        for e in entries {
            let Some(level) = e.level else { continue };
            if e.charging || level > icon::LOW_BATTERY + 5 {
                self.low_notified.remove(&e.key);
                continue;
            }
            if level > icon::LOW_BATTERY || self.low_notified.contains(&e.key) {
                continue;
            }
            if let Some(icon) = self.icons.iter().find(|i| i.key == e.key) {
                let body = t!("notify.low.body", name = e.name, level = level);
                // If it wasn't sent (e.g. Explorer is restarting), try again on the next refresh
                if platform::notify(&icon.tray, icon_guid(&e.key), &t!("notify.low.title"), &body) {
                    self.low_notified.insert(e.key.clone());
                }
            }
        }
    }

    /// Whether any device "should have a readable battery level" but is grayed out
    fn has_retryable_gray(&self) -> bool {
        self.entries.as_ref().is_some_and(|entries| {
            entries
                .iter()
                .any(|e| e.level.is_none() && (e.has_vendor_channel || self.seen_battery.contains(&e.key)))
        })
    }

    fn update_menu(&mut self, entries: &[Entry]) {
        for item in self.device_items.drain(..) {
            let _ = self.menu.remove(&item);
        }
        let mut new_items: Vec<MenuItem> = entries
            .iter()
            .map(|e| MenuItem::new(format!("{}{}{}", e.name, platform::MENU_SEPARATOR, e.status()), false, None))
            .collect();
        if new_items.is_empty() {
            new_items.push(MenuItem::new(t!("menu.no_devices"), false, None));
        }
        for (pos, item) in new_items.iter().enumerate() {
            let _ = self.menu.insert(item, pos);
        }
        self.device_items = new_items;
    }
}

fn main() {
    i18n::init();

    // Diagnostics: print battery levels once and exit (redirect the output, e.g. `bt-battery-tray --print > out.txt`)
    if std::env::args().any(|a| a == "--print") {
        for e in collect() {
            println!("{}\t{:?}\t{}", e.name, e.kind, e.status());
        }
        return;
    }

    // Diagnostics: print Bluetooth connection events received within 30 seconds (each connected device is reported once at startup)
    if std::env::args().any(|a| a == "--watch") {
        let (tx, rx) = mpsc::channel::<Trigger>();
        let _watcher = devwatch::ConnectionWatcher::start(tx, Trigger::DeviceChanged);
        let start = std::time::Instant::now();
        while rx.recv_timeout(Duration::from_secs(30).saturating_sub(start.elapsed())).is_ok() {
            println!("{:>6.1}s Bluetooth connection changed", start.elapsed().as_secs_f32());
        }
        return;
    }

    platform::init();
    let mut app = match App::new() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("failed to create tray icon: {e}");
            return;
        }
    };
    // Diagnostics: show a sample low-battery reminder to check whether system notifications are blocked
    if std::env::args().any(|a| a == "--notify-test") {
        let sent = app.placeholder.as_ref().is_some_and(|tray| {
            let body = t!("notify.low.body", name = t!("notify.sample_device"), level = 15);
            platform::notify(tray, icon_guid(PLACEHOLDER_KEY), &t!("notify.low.title"), &body)
        });
        println!("{}", if sent { "notification sent" } else { "notification failed" });
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(8) && platform::pump_events(Duration::from_millis(200)) {}
        return;
    }

    let (results_tx, results_rx): (Sender<Vec<Entry>>, Receiver<Vec<Entry>>) = mpsc::channel();
    let (trigger, trigger_rx) = mpsc::channel::<Trigger>();
    spawn_worker(results_tx, trigger_rx);
    // Refresh immediately when a Bluetooth device connects / disconnects; if watching fails, periodic refresh still works
    let _watcher = devwatch::ConnectionWatcher::start(trigger.clone(), Trigger::DeviceChanged).ok();
    let menu_events = MenuEvent::receiver();
    let tray_events = TrayIconEvent::receiver();
    let mut last_poll = Instant::now();
    let mut last_input_retry = Instant::now();

    loop {
        // Process tray events, checking the channels at least every 200ms
        if !platform::pump_events(Duration::from_millis(200)) {
            return;
        }

        while let Ok(event) = menu_events.try_recv() {
            if event.id == app.quit.id() {
                return;
            } else if event.id == app.refresh.id() {
                let _ = trigger.send(Trigger::Manual);
            } else if event.id == app.autostart.id() {
                let want = app.autostart.is_checked();
                if !autostart::set_enabled(want) {
                    app.autostart.set_checked(!want);
                }
            }
        }

        while let Ok(event) = tray_events.try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let _ = trigger.send(Trigger::Manual);
            }
        }

        // A sleeping keyboard stays connected, so there's no connection event; once the user types / moves the mouse the keyboard is awake, so retry grayed-out devices right away
        // Platforms without idle time (Linux) rely only on connection events and timed retries
        let input = platform::idle_time().is_some_and(|idle| idle < last_poll.elapsed());
        last_poll = Instant::now();
        if input && app.has_retryable_gray() && last_input_retry.elapsed() >= INPUT_RETRY_INTERVAL {
            last_input_retry = Instant::now();
            let _ = trigger.send(Trigger::Manual);
        }

        if let Some(entries) = results_rx.try_iter().last() {
            app.update(entries);
        }
    }
}
