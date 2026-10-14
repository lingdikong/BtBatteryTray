//! Reads accessory battery levels from the IOPowerSources accessory list.
//!
//! This is the source System Settings and Control Center read (`pmset -g accps` prints the same list): macOS publishes a
//! power source per Bluetooth accessory — AirPods (and their case), keyboards, mice — with the level already merged, e.g.
//! AirPods report the lower of the two buds. It covers the devices `system_profiler` and the IORegistry leave out:
//! since macOS 15 the HID stack no longer stores `BatteryPercent` for BLE peripherals.

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;

use super::bluetooth::BatteryReading;

/// `kIOPSSourceForAccessories`: only Bluetooth accessories, not the Mac's own battery
const ACCESSORY_SOURCES: i32 = 4;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesByType(source_type: i32) -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, source: CFTypeRef) -> CFDictionaryRef;
}

/// Battery levels of all Bluetooth accessories macOS tracks as power sources
pub fn batteries() -> Vec<BatteryReading> {
    // SAFETY: Copy rule, we own the blob; passing it on to the two getters below is what the API expects
    let blob = unsafe { IOPSCopyPowerSourcesByType(ACCESSORY_SOURCES) };
    if blob.is_null() {
        return Vec::new();
    }
    let blob = unsafe { CFType::wrap_under_create_rule(blob) };
    let list = unsafe { IOPSCopyPowerSourcesList(blob.as_CFTypeRef()) };
    if list.is_null() {
        return Vec::new();
    }
    let list: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(list) };
    let mut out = Vec::new();
    for source in list.iter() {
        let description = unsafe { IOPSGetPowerSourceDescription(blob.as_CFTypeRef(), source.as_CFTypeRef()) };
        if description.is_null() {
            continue;
        }
        // SAFETY: Get rule, the dictionary belongs to the blob
        let description: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_get_rule(description) };
        if let Some(reading) = read(&description) {
            out.push(reading);
        }
    }
    out
}

fn read(description: &CFDictionary<CFString, CFType>) -> Option<BatteryReading> {
    let value = |key: &str| description.find(CFString::new(key)).map(|v| v.clone());
    let number = |key: &str| value(key)?.downcast::<CFNumber>()?.to_i64();
    let flag = |key: &str| match value(key) {
        Some(v) if v.instance_of::<CFBoolean>() => Some(v.downcast::<CFBoolean>()?.into()),
        Some(v) => Some(v.downcast::<CFNumber>()?.to_i64()? != 0),
        None => None,
    };
    // A source is listed even while the accessory is away; "Is Present" is absent on devices that are always present
    if flag("Is Present") == Some(false) {
        return None;
    }
    let capacity = number("Current Capacity")?;
    // "Max Capacity" is 100 for every accessory seen so far, but the key exists so the level may be scaled
    let max = number("Max Capacity").filter(|m| *m > 0).unwrap_or(100);
    let product = value("Name")?.downcast::<CFString>()?.to_string();
    Some(BatteryReading {
        product: Some(product),
        ids: Vec::new(),
        level: (capacity * 100 / max).clamp(0, 100) as u8,
        charging: flag("Is Charging").unwrap_or(false),
    })
}
