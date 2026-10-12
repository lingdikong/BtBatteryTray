//! Enumerates connected Bluetooth devices via `system_profiler SPBluetoothDataType -json`.
//!
//! macOS has no public unified battery API; system_profiler reports the battery level of Apple devices, AirPods and most
//! BLE devices with a Battery Service (`device_batteryLevelMain` / `Left` / `Right`). The system doesn't expose iPhone battery level.

use std::process::Command;

use serde_json::Value;

use crate::bluetooth::{BtDevice, DeviceKind, normalize_address};

/// Returns all currently connected Bluetooth devices
pub fn connected_devices() -> Vec<BtDevice> {
    let output = Command::new("/usr/sbin/system_profiler")
        .args(["SPBluetoothDataType", "-json", "-detailLevel", "basic"])
        .output();
    match output.ok().and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok()) {
        Some(json) => parse(&json),
        None => Vec::new(),
    }
}

/// Structure: `{"SPBluetoothDataType": [{"device_connected": [{"<name>": {"device_address": ..., ...}}, ...]}]}`
fn parse(json: &Value) -> Vec<BtDevice> {
    let mut out = Vec::new();
    let controllers = json["SPBluetoothDataType"].as_array().into_iter().flatten();
    let connected = controllers.flat_map(|c| c["device_connected"].as_array().into_iter().flatten());
    for (name, info) in connected.filter_map(Value::as_object).flatten() {
        let Some(address) = info["device_address"].as_str().and_then(normalize_address) else { continue };
        let kind = info["device_minorType"]
            .as_str()
            .and_then(|t| DeviceKind::from_hint(&[t]))
            .unwrap_or(DeviceKind::Other);
        let percent = |key: &str| info[key].as_str()?.trim().trim_end_matches('%').parse::<u8>().ok();
        // For devices like AirPods that report left/right separately, take the lower one; the charging case doesn't count
        let battery = percent("device_batteryLevelMain")
            .or_else(|| [percent("device_batteryLevelLeft"), percent("device_batteryLevelRight")].into_iter().flatten().min())
            .or_else(|| percent("device_batteryLevel"))
            .map(|l| l.min(100));
        out.push(BtDevice { name: name.clone(), kind, address, battery, charging: false });
    }
    out
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
                    {"NX87": {"device_address": "B4:EE:25:2E:43:9F", "device_minorType": "Keyboard"}}
                ],
                "device_not_connected": [
                    {"Old": {"device_address": "11:22:33:44:55:66", "device_minorType": "Keyboard"}}
                ]
            }]}"#,
        )
        .unwrap();
        let devices = parse(&json);
        assert_eq!(devices.len(), 3);
        assert_eq!((devices[0].kind, devices[0].battery), (DeviceKind::Headphones, Some(75)));
        assert_eq!((devices[1].address.as_str(), devices[1].battery), ("C57858547FE5", Some(85)));
        assert_eq!((devices[2].kind, devices[2].battery), (DeviceKind::Keyboard, None));
    }
}
