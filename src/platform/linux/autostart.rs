//! Launch at login via the XDG autostart directory (~/.config/autostart/*.desktop)

use std::fs;
use std::path::PathBuf;

const FILE_NAME: &str = "bt-battery-tray.desktop";

fn desktop_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart").join(FILE_NAME))
}

/// Exec field of the desktop file: path is quoted, with `"` `` ` `` `$` `\` escaped
fn exec_line() -> Option<String> {
    // Under AppImage, current_exe points into a temporary mount; use the AppImage file itself
    let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())?;
    let mut quoted = String::from("\"");
    for c in exe.to_str()?.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    Some(format!("Exec={quoted}"))
}

pub fn is_enabled() -> bool {
    let (Some(path), Some(exec)) = (desktop_file(), exec_line()) else { return false };
    // If the program was moved, the old file counts as disabled; re-checking the option updates it
    fs::read_to_string(path).is_ok_and(|s| s.lines().any(|l| l == exec))
}

pub fn set_enabled(enable: bool) -> bool {
    let Some(path) = desktop_file() else { return false };
    if !enable {
        return fs::remove_file(&path).is_ok() || !path.exists();
    }
    let Some(exec) = exec_line() else { return false };
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Bluetooth Battery\nName[zh_CN]=蓝牙电量\nComment=Bluetooth device battery tray\nComment[zh_CN]=蓝牙设备电量托盘\n{exec}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    path.parent().is_some_and(|dir| fs::create_dir_all(dir).is_ok()) && fs::write(&path, content).is_ok()
}
