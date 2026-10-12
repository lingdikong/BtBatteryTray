//! Cross-platform Bluetooth device description; enumeration and battery reading live in `platform::*::bluetooth`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Headphones,
    Gamepad,
    Phone,
    Other,
}

impl DeviceKind {
    /// Guess the device kind from the type hints the system provides:
    /// Windows `DEVPKEY_DeviceContainer_Category` (`Input.Keyboard`, `Audio.Headphone`, `Communication.Phone`),
    /// BlueZ `Icon` (`input-keyboard`, `audio-headset`, `phone`), macOS `device_minorType` (`Keyboard`, `Headphones`)
    pub fn from_hint<S: AsRef<str>>(hints: &[S]) -> Option<Self> {
        hints.iter().find_map(|c| {
            let c = c.as_ref().to_ascii_lowercase();
            Some(match () {
                _ if c.contains("keyboard") => Self::Keyboard,
                _ if c.contains("mouse") || c.contains("touchpad") || c.contains("trackpad") => Self::Mouse,
                _ if c.contains("headphone") || c.contains("headset") || c.contains("audio") => Self::Headphones,
                // Must come after headphones: `headphone` also contains `phone`
                _ if c.contains("phone") => Self::Phone,
                _ if c.contains("gam") || c.contains("joystick") => Self::Gamepad,
                _ => return None,
            })
        })
    }

    /// Classic Bluetooth Class of Device: major class 0x05 peripheral / 0x04 audio/video / 0x02 phone
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn from_class_of_device(cod: u32) -> Option<Self> {
        let major = (cod >> 8) & 0x1f;
        let minor = (cod >> 2) & 0x3f;
        match major {
            0x05 if minor & 0x10 != 0 => Some(Self::Keyboard),
            0x05 if minor & 0x20 != 0 => Some(Self::Mouse),
            0x05 if minor & 0x0f == 0x02 || minor & 0x0f == 0x01 => Some(Self::Gamepad),
            0x04 => Some(Self::Headphones),
            0x02 => Some(Self::Phone),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BtDevice {
    pub name: String,
    pub kind: DeviceKind,
    /// Bluetooth address as 12 uppercase hex digits, used to match the vendor HID channel
    pub address: String,
    pub battery: Option<u8>,
    /// Can only be true when the system reports charging state (currently only Linux power_supply)
    pub charging: bool,
}

/// `b4:ee:25:2e:43:9f` / `B4-EE-25-2E-43-9F` / `b4ee252e439f` → `B4EE252E439F`
#[cfg_attr(windows, allow(dead_code))]
pub fn normalize_address(s: &str) -> Option<String> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let separators_only = s.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '-');
    (hex.len() == 12 && separators_only).then(|| hex.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(DeviceKind::from_hint(&["Audio.Headphone"]), Some(DeviceKind::Headphones));
        assert_eq!(DeviceKind::from_hint(&["Communication.Phone"]), Some(DeviceKind::Phone));
        assert_eq!(DeviceKind::from_hint(&["input-gaming"]), Some(DeviceKind::Gamepad));
        assert_eq!(DeviceKind::from_hint(&["Trackpad"]), Some(DeviceKind::Mouse));
        assert_eq!(DeviceKind::from_hint(&["computer"]), None);
    }

    #[test]
    fn addresses() {
        assert_eq!(normalize_address("b4:ee:25:2e:43:9f").as_deref(), Some("B4EE252E439F"));
        assert_eq!(normalize_address("B4-EE-25-2E-43-9F").as_deref(), Some("B4EE252E439F"));
        assert_eq!(normalize_address("hid-b4:ee:25:2e:43:9f-battery"), None);
        assert_eq!(normalize_address("1234"), None);
    }
}
