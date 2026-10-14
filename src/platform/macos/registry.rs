//! Reads the `BatteryPercent` property macOS publishes in the IORegistry for HID peripherals.
//!
//! For BLE devices with a Battery Service (and Apple's own peripherals), the HID stack stores the level on
//! `AppleDeviceManagementHIDEventService` / the IOHIDDevice (as shown by `ioreg -r -l -k BatteryPercent`), even when
//! system_profiler leaves it out, as it does for many third-party mice such as Logitech's.

use std::ffi::{CStr, c_char};

use core_foundation::base::{CFType, CFTypeRef, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::CFMutableDictionaryRef;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};

use super::bluetooth::HidBattery;
use crate::bluetooth::normalize_address;

type IoObject = u32;

const MAIN_PORT_DEFAULT: u32 = 0;
const ITERATE_RECURSIVELY: u32 = 1;
const ITERATE_PARENTS: u32 = 2;
const SERVICE_PLANE: &CStr = c"IOService";

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(main_port: u32, matching: CFMutableDictionaryRef, existing: *mut IoObject) -> i32;
    fn IOIteratorNext(iterator: IoObject) -> IoObject;
    fn IOObjectRelease(object: IoObject) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: IoObject, key: CFStringRef, allocator: CFTypeRef, options: u32) -> CFTypeRef;
    fn IORegistryEntrySearchCFProperty(
        entry: IoObject,
        plane: *const c_char,
        key: CFStringRef,
        allocator: CFTypeRef,
        options: u32,
    ) -> CFTypeRef;
}

/// Battery levels of all HID services that publish `BatteryPercent`
pub fn batteries() -> Vec<HidBattery> {
    let mut out: Vec<HidBattery> = Vec::new();
    for class in [c"AppleDeviceManagementHIDEventService", c"IOHIDDevice"] {
        for entry in services(class) {
            let Some(level) = own_property(entry, "BatteryPercent").and_then(|v| v.downcast::<CFNumber>()?.to_i64())
            else {
                unsafe { IOObjectRelease(entry) };
                continue;
            };
            let string = |key| search_property(entry, key)?.downcast::<CFString>().map(|s| s.to_string());
            let ids: Vec<String> =
                ["DeviceAddress", "SerialNumber"].into_iter().filter_map(|k| normalize_address(&string(k)?)).collect();
            let product = string("Product");
            unsafe { IOObjectRelease(entry) };
            // The event service and its IOHIDDevice may both carry the property
            if out.iter().any(|b| b.product == product && b.ids == ids) {
                continue;
            }
            out.push(HidBattery { product, ids, level: level.clamp(0, 100) as u8 });
        }
    }
    out
}

/// All registry entries of an IOService class (the caller releases each one)
fn services(class: &CStr) -> Vec<IoObject> {
    let mut out = Vec::new();
    unsafe {
        // IOServiceGetMatchingServices consumes the matching dictionary
        let matching = IOServiceMatching(class.as_ptr());
        let mut iterator: IoObject = 0;
        if matching.is_null() || IOServiceGetMatchingServices(MAIN_PORT_DEFAULT, matching, &mut iterator) != 0 {
            return out;
        }
        loop {
            let entry = IOIteratorNext(iterator);
            if entry == 0 {
                break;
            }
            out.push(entry);
        }
        IOObjectRelease(iterator);
    }
    out
}

fn own_property(entry: IoObject, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    // SAFETY: Create rule, we own the returned value
    let value = unsafe { IORegistryEntryCreateCFProperty(entry, key.as_concrete_TypeRef(), kCFAllocatorDefault as _, 0) };
    (!value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}

/// Looks the property up on the entry and then its ancestors (the address and name usually live on the IOHIDDevice above the event service)
fn search_property(entry: IoObject, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    // SAFETY: Create rule, we own the returned value
    let value = unsafe {
        IORegistryEntrySearchCFProperty(
            entry,
            SERVICE_PLANE.as_ptr(),
            key.as_concrete_TypeRef(),
            kCFAllocatorDefault as _,
            ITERATE_RECURSIVELY | ITERATE_PARENTS,
        )
    };
    (!value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}
