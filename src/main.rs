#![windows_subsystem = "windows"]

mod autostart;
mod bluetooth;
mod icon;
mod rongyuan;

use bluetooth::DeviceKind;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    WM_QUIT,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(240),
];

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    /// 蓝牙地址，用作托盘图标的身份
    key: String,
    name: String,
    kind: DeviceKind,
    level: Option<u8>,
    charging: bool,
}

impl Entry {
    fn status(&self) -> String {
        match self.level {
            Some(l) if self.charging => format!("{l}% 充电中"),
            Some(l) => format!("{l}%"),
            None => "未读取到电量".into(),
        }
    }
}

/// 汇总系统蓝牙电量和厂商私有协议电量
fn collect() -> Vec<Entry> {
    let devices = bluetooth::connected_devices();
    // 只有存在系统读不到电量的设备时才去碰厂商通道
    let vendor = if devices.iter().any(|d| d.battery.is_none()) {
        rongyuan::query_all()
    } else {
        Vec::new()
    };

    let mut used = vec![false; vendor.len()];
    let mut entries: Vec<Entry> = devices
        .into_iter()
        .map(|d| {
            if d.battery.is_none()
                && let Some(i) = vendor.iter().position(|v| v.matches_address(&d.address))
            {
                used[i] = true;
                return Entry { key: d.address, name: d.name, kind: d.kind, level: Some(vendor[i].level), charging: vendor[i].charging };
            }
            Entry { key: d.address, name: d.name, kind: d.kind, level: d.battery, charging: false }
        })
        .collect();
    // 理论上不会出现：厂商通道找得到但系统蓝牙列表里没有对应设备
    for (v, _) in vendor.iter().zip(&used).filter(|(_, u)| !**u) {
        // 荣源方案基本都是键盘
        entries.push(Entry {
            key: v.product.clone(),
            name: v.product.clone(),
            kind: DeviceKind::Keyboard,
            level: Some(v.level),
            charging: v.charging,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn spawn_worker(results: Sender<Vec<Entry>>) -> Sender<()> {
    let (trigger_tx, trigger_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut retry = 0;
        loop {
            let entries = collect();
            // 有已连接设备没读到电量（开机蓝牙未就绪、键盘刚唤醒等）时，按退避间隔尽快重试
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
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // 合并连续的刷新请求
            while trigger_rx.try_recv().is_ok() {}
        }
    });
    trigger_tx
}

struct App {
    menu: Menu,
    device_items: Vec<MenuItem>,
    refresh: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
    /// 每台已连接设备一个托盘图标（没读到电量的显示“未连接”样式），按 Entry::key 对应
    icons: Vec<(String, TrayIcon)>,
    /// 没有任何已连接设备时的占位图标，保证菜单仍可打开
    placeholder: Option<TrayIcon>,
    entries: Option<Vec<Entry>>,
    light_theme: bool,
}

/// 任务栏是否为浅色主题（HKCU\...\Themes\Personalize\SystemUsesLightTheme）
fn system_uses_light_theme() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 1
}

impl App {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let menu = Menu::new();
        let refresh = MenuItem::new("立即刷新", true, None);
        let autostart = CheckMenuItem::new("开机自启", true, autostart::is_enabled(), None);
        let quit = MenuItem::new("退出", true, None);
        let loading = MenuItem::new("正在读取…", false, None);
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
            light_theme: system_uses_light_theme(),
        };
        app.placeholder = Some(app.build_icon(None, false, DeviceKind::Other, "蓝牙电量：正在读取…")?);
        Ok(app)
    }

    fn render(&self, level: Option<u8>, charging: bool, kind: DeviceKind) -> tray_icon::Icon {
        icon::render(level, charging, kind, icon::Style { light_theme: self.light_theme })
    }

    fn build_icon(&self, level: Option<u8>, charging: bool, kind: DeviceKind, tip: &str) -> tray_icon::Result<TrayIcon> {
        TrayIconBuilder::new()
            .with_icon(self.render(level, charging, kind))
            .with_tooltip(tip)
            .with_menu(Box::new(self.menu.clone()))
            // 左键用来刷新，菜单只在右键弹出
            .with_menu_on_left_click(false)
            .build()
    }

    fn update(&mut self, entries: Vec<Entry>) {
        // 主题切换后即使数据没变也要重画
        let light_theme = system_uses_light_theme();
        // 上次有图标创建失败时，即使数据没变也要重试
        let all_icons_present = entries.len() == self.icons.len();
        if self.entries.as_ref() == Some(&entries) && self.light_theme == light_theme && all_icons_present {
            return;
        }
        self.light_theme = light_theme;
        self.update_menu(&entries);

        // 断开的设备移除图标（TrayIcon drop 时自动从托盘删除）
        self.icons.retain(|(key, _)| entries.iter().any(|e| &e.key == key));
        for e in &entries {
            // 托盘提示最长 127 个字符
            let tip: String = format!("{}\n{}", e.name, e.status()).chars().take(127).collect();
            if let Some((_, tray)) = self.icons.iter().find(|(key, _)| key == &e.key) {
                let _ = tray.set_icon(Some(self.render(e.level, e.charging, e.kind)));
                let _ = tray.set_tooltip(Some(tip));
            } else if let Ok(tray) = self.build_icon(e.level, e.charging, e.kind, &tip) {
                self.icons.push((e.key.clone(), tray));
            }
        }

        if self.icons.is_empty() {
            let tip = "蓝牙电量：没有已连接的设备";
            match &self.placeholder {
                Some(tray) => {
                    let _ = tray.set_icon(Some(self.render(None, false, DeviceKind::Other)));
                    let _ = tray.set_tooltip(Some(tip));
                }
                None => self.placeholder = self.build_icon(None, false, DeviceKind::Other, tip).ok(),
            }
        } else {
            self.placeholder = None;
        }
        self.entries = Some(entries);
    }

    fn update_menu(&mut self, entries: &[Entry]) {
        for item in self.device_items.drain(..) {
            let _ = self.menu.remove(&item);
        }
        let mut new_items: Vec<MenuItem> = entries
            .iter()
            .map(|e| MenuItem::new(format!("{}\t{}", e.name, e.status()), false, None))
            .collect();
        if new_items.is_empty() {
            new_items.push(MenuItem::new("没有已连接的蓝牙设备", false, None));
        }
        for (pos, item) in new_items.iter().enumerate() {
            let _ = self.menu.insert(item, pos);
        }
        self.device_items = new_items;
    }
}

fn main() {
    // 诊断用：打印一次电量后退出（输出需重定向，如 `bt-battery-tray --print > out.txt`）
    if std::env::args().any(|a| a == "--print") {
        for e in collect() {
            println!("{}\t{:?}\t{}", e.name, e.kind, e.status());
        }
        return;
    }

    let mut app = match App::new() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("创建托盘图标失败: {e}");
            return;
        }
    };
    let (results_tx, results_rx): (Sender<Vec<Entry>>, Receiver<Vec<Entry>>) = mpsc::channel();
    let trigger = spawn_worker(results_tx);
    let menu_events = MenuEvent::receiver();
    let tray_events = TrayIconEvent::receiver();

    loop {
        // 等待窗口消息，最多 200ms 后检查一次通道
        unsafe { MsgWaitForMultipleObjects(None, false, 200, QS_ALLINPUT) };
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(event) = menu_events.try_recv() {
            if event.id == app.quit.id() {
                return;
            } else if event.id == app.refresh.id() {
                let _ = trigger.send(());
            } else if event.id == app.autostart.id() {
                let want = app.autostart.is_checked();
                if !autostart::set_enabled(want) {
                    app.autostart.set_checked(!want);
                }
            }
        }

        while let Ok(event) = tray_events.try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let _ = trigger.send(());
            }
        }

        if let Some(entries) = results_rx.try_iter().last() {
            app.update(entries);
        }
    }
}
