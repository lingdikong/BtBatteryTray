//! Launch at login via a LaunchAgent (~/Library/LaunchAgents/*.plist)

use std::fs;
use std::path::PathBuf;

const LABEL: &str = "com.lingdikong.btbatterytray";

fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// Program path for the plist (XML-escaped)
fn program() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.to_str()?.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"))
}

pub fn is_enabled() -> bool {
    let (Some(path), Some(program)) = (plist_path(), program()) else { return false };
    // If the program was moved, the old file counts as disabled; re-checking the option updates it
    fs::read_to_string(path).is_ok_and(|s| s.contains(&format!("<string>{program}</string>")))
}

pub fn set_enabled(enable: bool) -> bool {
    let Some(path) = plist_path() else { return false };
    if !enable {
        return fs::remove_file(&path).is_ok() || !path.exists();
    }
    let Some(program) = program() else { return false };
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    );
    path.parent().is_some_and(|dir| fs::create_dir_all(dir).is_ok()) && fs::write(&path, content).is_ok()
}
