pub mod autostart;
pub mod bluetooth;
pub mod devwatch;
pub mod hid;

use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
    WM_QUIT,
};

/// Tray icon side length (pixels)
pub const ICON_SIZE: usize = 32;
/// Separator between device name and battery level in the menu: Windows menus right-align text after `\t`
pub const MENU_SEPARATOR: &str = "\t";

/// Tray-related initialization (not needed on Windows)
pub fn init() {}

/// Processes window messages, waiting at most `timeout`; returns false on a quit message
pub fn pump_events(timeout: Duration) -> bool {
    unsafe { MsgWaitForMultipleObjects(None, false, timeout.as_millis() as u32, QS_ALLINPUT) };
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            return false;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

/// Sends a notification via a tray balloon notification: Windows 10/11 shows it as a system toast, no AppUserModelID registration needed.
/// The icon carries a GUID, which identifies the matching tray icon. Returns whether the system accepted the notification.
pub fn notify(tray: &tray_icon::TrayIcon, guid: u128, title: &str, body: &str) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{NIF_GUID, NIF_INFO, NIIF_WARNING, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW};
    use windows::core::GUID;
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(tray.window_handle() as _),
        uFlags: NIF_INFO | NIF_GUID,
        guidItem: GUID::from_u128(guid),
        dwInfoFlags: NIIF_WARNING,
        ..Default::default()
    };
    copy_wide(&mut nid.szInfoTitle, title);
    copy_wide(&mut nid.szInfo, body);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) }.as_bool()
}

/// Copies into a fixed-length UTF-16 buffer with truncation, keeping the trailing 0
fn copy_wide(dst: &mut [u16], s: &str) {
    let limit = dst.len() - 1;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(limit)) {
        *d = c;
    }
}

/// Whether the taskbar uses a light theme (HKCU\...\Themes\Personalize\SystemUsesLightTheme)
pub fn light_theme() -> bool {
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

/// Time since the last keyboard/mouse input (GetLastInputInfo); only reads a system counter, does not touch devices
pub fn idle_time() -> Option<Duration> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    Some(Duration::from_millis(unsafe { GetTickCount() }.wrapping_sub(info.dwTime) as u64))
}
