//! Enumerates connected Bluetooth devices through BlueZ's D-Bus interface.
//!
//! Battery level comes from BlueZ `org.bluez.Battery1` first (BLE Battery Service, HFP headsets); otherwise falls back to the kernel's
//! `/sys/class/power_supply` (reported by drivers such as Logitech HID++ and the HID battery usage, with charging state too).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use zbus::blocking::Connection;
use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::OwnedValue;

use crate::bluetooth::{BtDevice, DeviceKind, normalize_address};

type Properties = HashMap<String, OwnedValue>;

/// Returns all currently connected Bluetooth devices
pub fn connected_devices() -> Vec<BtDevice> {
    let objects = (|| -> zbus::Result<_> {
        let conn = Connection::system()?;
        let proxy = ObjectManagerProxy::builder(&conn).destination("org.bluez")?.path("/")?.build()?;
        Ok(proxy.get_managed_objects()?)
    })();
    let Ok(objects) = objects else { return Vec::new() };
    let supplies = power_supplies();

    let mut out = Vec::new();
    for ifaces in objects.values() {
        let Some(dev) = interface(ifaces, "org.bluez.Device1") else { continue };
        if get::<bool>(dev, "Connected") != Some(true) {
            continue;
        }
        let Some(address) = get::<String>(dev, "Address").and_then(|a| normalize_address(&a)) else { continue };
        let name = get::<String>(dev, "Alias").or_else(|| get(dev, "Name")).unwrap_or_else(|| address.clone());
        let kind = get::<String>(dev, "Icon")
            .and_then(|icon| DeviceKind::from_hint(&[icon]))
            .or_else(|| get::<u32>(dev, "Class").and_then(DeviceKind::from_class_of_device))
            .unwrap_or(DeviceKind::Other);
        let supply = supplies.iter().find(|s| s.address == address);
        let battery = interface(ifaces, "org.bluez.Battery1")
            .and_then(|b| get::<u8>(b, "Percentage"))
            .map(|p| p.min(100))
            .or(supply.map(|s| s.level));
        let charging = supply.is_some_and(|s| s.charging);
        out.push(BtDevice { name, kind, address, battery, charging });
    }
    out
}

fn interface<'a>(ifaces: &'a HashMap<OwnedInterfaceName, Properties>, name: &str) -> Option<&'a Properties> {
    ifaces.iter().find(|(k, _)| k.as_str() == name).map(|(_, v)| v)
}

fn get<T: TryFrom<OwnedValue>>(props: &Properties, key: &str) -> Option<T> {
    props.get(key)?.try_clone().ok()?.try_into().ok()
}

struct PowerSupply {
    address: String,
    level: u8,
    charging: bool,
}

/// power_supply entries the kernel creates for peripheral batteries (`scope` is `Device`), matched by Bluetooth address
fn power_supplies() -> Vec<PowerSupply> {
    let Ok(dir) = fs::read_dir("/sys/class/power_supply") else { return Vec::new() };
    dir.flatten()
        .filter_map(|entry| {
            let path = entry.path();
            // Skip the laptop's own battery and the AC adapter
            if read(&path.join("scope")).as_deref() != Some("Device") {
                return None;
            }
            // A HID device's HID_UNIQ is its Bluetooth address; generic HID batteries are named like `hid-b4:ee:..:9f-battery`
            let address = read(&path.join("device/uevent"))
                .and_then(|u| u.lines().find_map(|l| l.strip_prefix("HID_UNIQ=").and_then(normalize_address)))
                .or_else(|| entry.file_name().to_str()?.split(['-', '_']).find_map(normalize_address))?;
            let level = read(&path.join("capacity"))?.parse::<u8>().ok()?.min(100);
            let charging = read(&path.join("status")).as_deref() == Some("Charging");
            Some(PowerSupply { address, level, charging })
        })
        .collect()
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}
