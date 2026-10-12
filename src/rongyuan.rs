//! Private battery protocol of Bluetooth keyboards built on the RongYuan (VID 0x3151) platform.
//!
//! Many keyboards (Feker NX87, Akko, MonsGeek, Epomaker, ...) run this firmware. Over Classic/BLE,
//! the keyboard exposes a vendor HID collection (UsagePage 0xFF66 / 0xFF55, Usage 0x0202, Report ID 6)
//! that sends and receives 66-byte reports:
//!
//! ```text
//! Request:  [06] [55] [cmd] [d1..d6] [checksum] [0...]   checksum = 0xFF - (cmd+d1..d6)
//! Response: [06] [55] [cmd] [r1] [r2] ...
//! ```
//!
//! In the `GET_BATTERY (0x83)` response, r1 = battery percentage, r2 = 0 not charging / 1 charging / 2 full.
//! Protocol reference: github.com/echtzeit-solutions/monsgeek-akko-linux and the official driver's frontend code.
//!
//! Only the protocol lives here; finding devices and exchanging reports is done in `platform::*::hid`.

use std::time::Duration;

use crate::platform::hid;

pub const VID_RONGYUAN: u16 = 0x3151;
pub const VENDOR_USAGE_PAGES: [u16; 2] = [0xFF66, 0xFF55];
pub const VENDOR_USAGE: u16 = 0x0202;
pub const REPORT_LEN: usize = 66;
const REPORT_ID: u8 = 0x06;
const CMD_MARKER: u8 = 0x55;
const CMD_GET_BATTERY: u8 = 0x83;

#[derive(Debug, Clone)]
pub struct VendorBattery {
    pub product: String,
    /// ID strings containing the Bluetooth address (12 uppercase hex digits), used to match the system's Bluetooth device list
    pub ids: Vec<String>,
    /// None: the channel exists but the keyboard didn't respond (asleep, just connected and not ready yet, etc.)
    pub level: Option<u8>,
    pub charging: bool,
}

impl VendorBattery {
    pub fn matches_address(&self, address: &str) -> bool {
        self.ids.iter().any(|id| id.contains(address))
    }
}

/// Query the battery level of all RongYuan Bluetooth devices (devices whose query fails are returned too, with level None)
pub fn query_all() -> Vec<VendorBattery> {
    hid::vendor_interfaces().into_iter().map(query).collect()
}

fn query(iface: hid::VendorInterface) -> VendorBattery {
    let dev = iface.open();
    let req = request(CMD_GET_BATTERY);
    let accept = |r: &[u8]| r.len() >= 5 && r[0] == REPORT_ID && r[1] == CMD_MARKER && r[2] == CMD_GET_BATTERY;
    // The keyboard occasionally doesn't respond right after waking or while busy; try once more
    let resp = dev.as_ref().and_then(|d| {
        d.transact(&req, &accept, Duration::from_millis(800))
            .or_else(|| d.transact(&req, &accept, Duration::from_millis(1500)))
    });
    VendorBattery {
        product: iface.product.clone().unwrap_or_else(|| "RongYuan keyboard".into()),
        ids: iface.ids.clone(),
        level: resp.map(|r| r[3].min(100)),
        charging: resp.is_some_and(|r| r[4] == 1),
    }
}

/// Request report for a command without parameters
fn request(cmd: u8) -> [u8; REPORT_LEN] {
    let mut req = [0u8; REPORT_LEN];
    req[0] = REPORT_ID;
    req[1] = CMD_MARKER;
    req[2] = cmd;
    req[9] = 0xFF - (req[2..9].iter().map(|&b| b as u32).sum::<u32>() & 0xFF) as u8;
    req
}

/// Whether the HID report descriptor contains the RongYuan vendor collection (Linux / macOS get the whole device's descriptor, so we have to search it ourselves)
#[cfg_attr(windows, allow(dead_code))]
pub fn descriptor_has_vendor_collection(desc: &[u8]) -> bool {
    let mut usage_page = 0u16;
    let mut usages: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        // Long item: 0xFE, data size, tag, data...
        if prefix == 0xFE {
            i += 3 + desc.get(i + 1).copied().unwrap_or(0) as usize;
            continue;
        }
        let size = [0, 1, 2, 4][(prefix & 0x03) as usize];
        let Some(data) = desc.get(i + 1..i + 1 + size) else { break };
        let value = data.iter().rev().fold(0u32, |v, &b| v << 8 | b as u32);
        i += 1 + size;
        match prefix & 0xFC {
            // Usage Page (global)
            0x04 => usage_page = value as u16,
            // Usage (local); a 4-byte usage carries its own Usage Page in the high 16 bits
            0x08 => usages.push(if size == 4 { value } else { (usage_page as u32) << 16 | value }),
            // Collection
            0xA0 => {
                let hit = usages.last().is_some_and(|&u| {
                    VENDOR_USAGE_PAGES.contains(&((u >> 16) as u16)) && u & 0xFFFF == VENDOR_USAGE as u32
                });
                if hit {
                    return true;
                }
                usages.clear();
            }
            // Other Main items (Input / Output / Feature / End Collection) clear the local state
            0x80 | 0x90 | 0xB0 | 0xC0 => usages.clear(),
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum() {
        let req = request(CMD_GET_BATTERY);
        assert_eq!(&req[..10], &[0x06, 0x55, 0x83, 0, 0, 0, 0, 0, 0, 0x7C]);
    }

    #[test]
    fn finds_vendor_collection() {
        // Keyboard collection + vendor collection (Usage Page 0xFF66, Usage 0x0202)
        let desc = [
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0xC0, // Generic Desktop / Keyboard
            0x06, 0x66, 0xFF, 0x0A, 0x02, 0x02, 0xA1, 0x01, 0x85, 0x06, 0xC0,
        ];
        assert!(descriptor_has_vendor_collection(&desc));
        assert!(!descriptor_has_vendor_collection(&desc[..7]));
    }
}
