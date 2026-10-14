//! Enumerates connected Bluetooth devices via `system_profiler SPBluetoothDataType -json`.
//!
//! macOS has no public unified battery API; system_profiler reports the battery level of Apple devices, AirPods and some
//! BLE devices with a Battery Service (`device_batteryLevelMain` / `Left` / `Right`). The system doesn't expose iPhone battery level.
//!
//! Devices it reports no level for (third-party keyboards and mice such as Logitech's) fall back to the accessory power
//! sources macOS publishes for System Settings and Control Center, and then to the IORegistry `BatteryPercent` property
//! the HID stack used to carry before macOS 15.

use std::process::Command;

use rust_i18n::t;
use serde_json::Value;

use crate::bluetooth::{BtDevice, DeviceKind, normalize_address};

/// Battery level read outside the Bluetooth list, to be matched back to a device in it
pub struct BatteryReading {
    pub product: Option<String>,
    /// Bluetooth addresses found on the device (12 uppercase hex digits); empty for power sources, which carry no address
    pub ids: Vec<String>,
    pub level: u8,
    pub charging: bool,
}

/// Returns all currently connected Bluetooth devices
pub fn connected_devices() -> Vec<BtDevice> {
    let output = Command::new("/usr/sbin/system_profiler")
        .args(["SPBluetoothDataType", "-json", "-detailLevel", "basic"])
        .output();
    let mut devices = match output.ok().and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok()) {
        Some(json) => parse(&json),
        None => Vec::new(),
    };
    if devices.iter().any(|d| d.battery.is_none()) {
        fill(&mut devices, super::powersource::batteries());
    }
    if devices.iter().any(|d| d.battery.is_none()) {
        fill(&mut devices, super::registry::batteries());
    }
    devices
}

/// Fills in missing levels, matching by Bluetooth address first and then by product name
fn fill(devices: &mut [BtDevice], batteries: Vec<BatteryReading>) {
    let mut used = vec![false; batteries.len()];
    for d in devices.iter_mut().filter(|d| d.battery.is_none()) {
        let by_address = || batteries.iter().position(|b| b.ids.contains(&d.address));
        let by_name = || {
            batteries
                .iter()
                .zip(&used)
                .position(|(b, u)| !*u && b.product.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(&d.name)))
        };
        if let Some(i) = by_address().or_else(by_name) {
            used[i] = true;
            d.battery = Some(batteries[i].level);
            d.charging = batteries[i].charging;
        }
    }
}

/// Structure: `{"SPBluetoothDataType": [{"device_connected": [{"<name>": {"device_address": ..., ...}}, ...]}]}`
fn parse(json: &Value) -> Vec<BtDevice> {
    let mut out = Vec::new();
    let mut cases = Vec::new();
    let controllers = json["SPBluetoothDataType"].as_array().into_iter().flatten();
    let connected = controllers.flat_map(|c| c["device_connected"].as_array().into_iter().flatten());
    for (name, info) in connected.filter_map(Value::as_object).flatten() {
        let Some(address) = info["device_address"].as_str().and_then(normalize_address) else { continue };
        let kind = info["device_minorType"]
            .as_str()
            .and_then(|t| DeviceKind::from_hint(&[t]))
            .unwrap_or(DeviceKind::Other);
        let percent = |key: &str| info[key].as_str()?.trim().trim_end_matches('%').parse::<u8>().ok();
        // Only buds report their two sides and a case of their own; over-ear headphones report one level
        let sides = ["device_batteryLevelLeft", "device_batteryLevelRight", "device_batteryLevelCase"];
        let kind = match kind {
            DeviceKind::Headphones if sides.iter().any(|k| !info[*k].is_null()) => DeviceKind::Earbuds,
            kind => kind,
        };
        // For devices like AirPods that report left/right separately, take the lower one; the charging case doesn't count
        let battery = percent("device_batteryLevelMain")
            .or_else(|| [percent("device_batteryLevelLeft"), percent("device_batteryLevelRight")].into_iter().flatten().min())
            .or_else(|| percent("device_batteryLevel"))
            .map(|l| l.min(100));
        if let Some(level) = percent("device_batteryLevelCase") {
            cases.push((name.clone(), level.min(100)));
        }
        out.push(BtDevice { name: name.clone(), kind, address, battery, charging: false });
    }
    drop_ble_shadows(&mut out);
    append_cases(&mut out, cases);
    out
}

/// AirPods report their charging case separately and macOS lists it as an accessory of its own, so give it its own entry
fn append_cases(devices: &mut Vec<BtDevice>, cases: Vec<(String, u8)>) {
    for (name, level) in cases {
        // Both records of a pair of AirPods carry the case level: hang the case off the one that survived deduplication
        let Some(address) = devices.iter().find(|d| d.name == name).map(|d| format!("{}-case", d.address)) else {
            continue;
        };
        if devices.iter().any(|d| d.address == address) {
            continue;
        }
        devices.push(BtDevice {
            name: t!("device.case", name = name).into_owned(),
            kind: DeviceKind::EarbudsCase,
            address,
            battery: Some(level),
            charging: false,
        });
    }
}

/// AirPods are listed twice: as the audio device and again under their BLE address, a record that carries no type and no
/// level of its own (only the case's). Drop that shadow so the device isn't shown a second time as "unknown".
fn drop_ble_shadows(devices: &mut Vec<BtDevice>) {
    let shadows: Vec<bool> = devices
        .iter()
        .enumerate()
        .map(|(i, d)| {
            d.kind == DeviceKind::Other
                && d.battery.is_none()
                && devices.iter().enumerate().any(|(j, other)| j != i && other.name == d.name)
        })
        .collect();
    let mut shadows = shadows.into_iter();
    devices.retain(|_| !shadows.next().unwrap_or(false));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connected_devices() {
        let json: Value = serde_json::from_str(
            r#"{"SPBluetoothDataType": [{
                "controller_properties": {"controller_address": "AA:BB:CC:DD:EE:FF"},
                "device_connected": [
                    {"AirPods Pro": {"device_address": "40:DA:5C:5A:76:B6", "device_batteryLevelLeft": "80%",
                                     "device_batteryLevelRight": "75%", "device_batteryLevelCase": "20%",
                                     "device_minorType": "Headphones"}},
                    {"MX Anywhere 3": {"device_address": "C5:78:58:54:7F:E5", "device_batteryLevelMain": "85%",
                                       "device_minorType": "Mouse"}},
                    {"NX87": {"device_address": "B4:EE:25:2E:43:9F", "device_minorType": "Keyboard"}},
                    {"AirPods Pro": {"device_address": "F4:70:D6:CF:90:06", "device_batteryLevelCase": "61%"}}
                ],
                "device_not_connected": [
                    {"Old": {"device_address": "11:22:33:44:55:66", "device_minorType": "Keyboard"}}
                ]
            }]}"#,
        )
        .unwrap();
        let devices = parse(&json);
        // The AirPods' BLE shadow is dropped and their charging case is appended as a device of its own
        assert_eq!(devices.len(), 4);
        assert_eq!((devices[0].kind, devices[0].battery), (DeviceKind::Earbuds, Some(75)));
        assert_eq!((devices[1].address.as_str(), devices[1].battery), ("C57858547FE5", Some(85)));
        assert_eq!((devices[2].kind, devices[2].battery), (DeviceKind::Keyboard, None));
        assert_eq!((devices[3].kind, devices[3].battery), (DeviceKind::EarbudsCase, Some(20)));
        assert_eq!(devices[3].address, "40DA5C5A76B6-case");
    }

    #[test]
    fn fills_missing_levels() {
        let device = |name: &str, address: &str, battery| BtDevice {
            name: name.into(),
            kind: DeviceKind::Mouse,
            address: address.into(),
            battery,
            charging: false,
        };
        let mut devices = vec![
            device("MX Master 3S", "C57858547FE5", None),
            device("MX Keys", "D1D2D3D4D5D6", None),
            device("Magic Mouse", "A1A2A3A4A5A6", Some(90)),
        ];
        let batteries = vec![
            BatteryReading { product: Some("mx keys".into()), ids: vec![], level: 40, charging: false },
            BatteryReading { product: Some("Other".into()), ids: vec!["C57858547FE5".into()], level: 70, charging: false },
            BatteryReading { product: Some("Magic Mouse".into()), ids: vec![], level: 10, charging: true },
        ];
        fill(&mut devices, batteries);
        assert_eq!(devices[0].battery, Some(70));
        assert_eq!(devices[1].battery, Some(40));
        assert_eq!(devices[2].battery, Some(90));
        assert!(!devices[2].charging);
    }
}
