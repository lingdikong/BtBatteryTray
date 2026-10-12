//! On macOS, finds the RongYuan vendor collection and exchanges reports via IOKit HID.
//!
//! As on Linux, one IOHIDDevice covers the whole device, so the report descriptor must be read to know whether it has the vendor collection.
//! Opening keyboard-class devices requires the user to allow this app under "Privacy & Security → Input Monitoring".

use std::ffi::c_void;
use std::time::{Duration, Instant};

use core_foundation::base::{CFIndex, CFRelease, CFRetain, CFType, TCFType, kCFAllocatorDefault};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::runloop::{CFRunLoopGetCurrent, CFRunLoopRunInMode, kCFRunLoopDefaultMode};
use core_foundation::set::{CFSetGetCount, CFSetGetValues};
use core_foundation::string::CFString;

use crate::bluetooth::normalize_address;
use crate::rongyuan::{REPORT_LEN, VID_RONGYUAN, descriptor_has_vendor_collection};

pub(super) mod ffi {
    #![allow(non_upper_case_globals)]

    use std::ffi::c_void;

    use core_foundation::array::CFArrayRef;
    use core_foundation::base::{CFAllocatorRef, CFIndex, CFTypeRef};
    use core_foundation::dictionary::CFDictionaryRef;
    use core_foundation::runloop::CFRunLoopRef;
    use core_foundation::set::CFSetRef;
    use core_foundation::string::CFStringRef;

    pub type IOHIDManagerRef = *mut c_void;
    pub type IOHIDDeviceRef = *mut c_void;
    pub type IOReturn = i32;
    pub type IOHIDReportType = u32;

    pub const kIOReturnSuccess: IOReturn = 0;
    pub const kIOHIDOptionsTypeNone: u32 = 0;
    pub const kIOHIDReportTypeOutput: IOHIDReportType = 1;

    pub type IOHIDReportCallback = unsafe extern "C" fn(
        context: *mut c_void,
        result: IOReturn,
        sender: *mut c_void,
        report_type: IOHIDReportType,
        report_id: u32,
        report: *mut u8,
        report_length: CFIndex,
    );
    pub type IOHIDDeviceCallback =
        unsafe extern "C" fn(context: *mut c_void, result: IOReturn, sender: *mut c_void, device: IOHIDDeviceRef);

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        pub fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
        pub fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
        pub fn IOHIDManagerSetDeviceMatchingMultiple(manager: IOHIDManagerRef, multiple: CFArrayRef);
        pub fn IOHIDManagerCopyDevices(manager: IOHIDManagerRef) -> CFSetRef;
        pub fn IOHIDManagerRegisterDeviceMatchingCallback(
            manager: IOHIDManagerRef,
            callback: IOHIDDeviceCallback,
            context: *mut c_void,
        );
        pub fn IOHIDManagerRegisterDeviceRemovalCallback(
            manager: IOHIDManagerRef,
            callback: IOHIDDeviceCallback,
            context: *mut c_void,
        );
        pub fn IOHIDManagerScheduleWithRunLoop(manager: IOHIDManagerRef, run_loop: CFRunLoopRef, mode: CFStringRef);

        pub fn IOHIDDeviceGetProperty(device: IOHIDDeviceRef, key: CFStringRef) -> CFTypeRef;
        pub fn IOHIDDeviceOpen(device: IOHIDDeviceRef, options: u32) -> IOReturn;
        pub fn IOHIDDeviceClose(device: IOHIDDeviceRef, options: u32) -> IOReturn;
        pub fn IOHIDDeviceSetReport(
            device: IOHIDDeviceRef,
            report_type: IOHIDReportType,
            report_id: CFIndex,
            report: *const u8,
            report_length: CFIndex,
        ) -> IOReturn;
        pub fn IOHIDDeviceRegisterInputReportCallback(
            device: IOHIDDeviceRef,
            report: *mut u8,
            report_length: CFIndex,
            callback: Option<IOHIDReportCallback>,
            context: *mut c_void,
        );
        pub fn IOHIDDeviceScheduleWithRunLoop(device: IOHIDDeviceRef, run_loop: CFRunLoopRef, mode: CFStringRef);
        pub fn IOHIDDeviceUnscheduleFromRunLoop(device: IOHIDDeviceRef, run_loop: CFRunLoopRef, mode: CFStringRef);
    }
}

use ffi::*;

/// Holds one reference count on an IOHIDDevice
struct DeviceRef(IOHIDDeviceRef);

impl DeviceRef {
    /// SAFETY: `dev` must be a valid IOHIDDevice
    unsafe fn retain(dev: IOHIDDeviceRef) -> Self {
        unsafe { CFRetain(dev as _) };
        Self(dev)
    }

    fn property(&self, key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        // SAFETY: Get rule, we don't own the return value, so retain it before use
        let value = unsafe { IOHIDDeviceGetProperty(self.0, key.as_concrete_TypeRef()) };
        (!value.is_null()).then(|| unsafe { CFType::wrap_under_get_rule(value) })
    }

    fn string(&self, key: &str) -> Option<String> {
        self.property(key)?.downcast::<CFString>().map(|s| s.to_string())
    }
}

impl Drop for DeviceRef {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0 as _) };
    }
}

pub struct VendorInterface {
    pub product: Option<String>,
    /// A Bluetooth HID device's SerialNumber is its Bluetooth address
    pub ids: Vec<String>,
    device: DeviceRef,
}

impl VendorInterface {
    pub fn open(&self) -> Option<Device> {
        // SAFETY: device is valid
        let status = unsafe { IOHIDDeviceOpen(self.device.0, kIOHIDOptionsTypeNone) };
        (status == kIOReturnSuccess).then(|| Device(unsafe { DeviceRef::retain(self.device.0) }))
    }
}

pub struct Device(DeviceRef);

impl Drop for Device {
    fn drop(&mut self) {
        unsafe { IOHIDDeviceClose(self.0.0, kIOHIDOptionsTypeNone) };
    }
}

struct ReportContext<'a> {
    accept: &'a dyn Fn(&[u8]) -> bool,
    response: Option<[u8; REPORT_LEN]>,
}

unsafe extern "C" fn on_input_report(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    _report_type: IOHIDReportType,
    report_id: u32,
    report: *mut u8,
    report_length: CFIndex,
) {
    // SAFETY: context is the ReportContext still alive in transact
    let ctx = unsafe { &mut *(context as *mut ReportContext) };
    if ctx.response.is_some() || report.is_null() {
        return;
    }
    let data = unsafe { std::slice::from_raw_parts(report, report_length.max(0) as usize) };
    // IOKit may deliver reports without the Report ID; normalize to the with-ID form (the reply's second byte is 0x55, so it can't be confused with the ID)
    let mut full = Vec::with_capacity(data.len() + 1);
    if data.first() != Some(&(report_id as u8)) {
        full.push(report_id as u8);
    }
    full.extend_from_slice(data);
    if (ctx.accept)(&full) {
        let mut resp = [0u8; REPORT_LEN];
        let len = full.len().min(REPORT_LEN);
        resp[..len].copy_from_slice(&full[..len]);
        ctx.response = Some(resp);
    }
}

impl Device {
    /// Sends a report and waits on the current thread's run loop for the first reply that `accept` approves
    pub fn transact(
        &self,
        req: &[u8; REPORT_LEN],
        accept: &dyn Fn(&[u8]) -> bool,
        timeout: Duration,
    ) -> Option<[u8; REPORT_LEN]> {
        let dev = self.0.0;
        let mut buf = vec![0u8; 256];
        let mut ctx = ReportContext { accept, response: None };
        unsafe {
            let run_loop = CFRunLoopGetCurrent();
            IOHIDDeviceScheduleWithRunLoop(dev, run_loop, kCFRunLoopDefaultMode);
            IOHIDDeviceRegisterInputReportCallback(
                dev,
                buf.as_mut_ptr(),
                buf.len() as CFIndex,
                Some(on_input_report),
                &mut ctx as *mut ReportContext as *mut c_void,
            );
            // Register the callback before sending so the reply can't be missed; numbered report data keeps the Report ID
            let sent = IOHIDDeviceSetReport(dev, kIOHIDReportTypeOutput, req[0] as CFIndex, req.as_ptr(), req.len() as CFIndex)
                == kIOReturnSuccess;
            let deadline = Instant::now() + timeout;
            while sent && ctx.response.is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, left.as_secs_f64(), 1);
            }
            IOHIDDeviceRegisterInputReportCallback(dev, buf.as_mut_ptr(), buf.len() as CFIndex, None, std::ptr::null_mut());
            IOHIDDeviceUnscheduleFromRunLoop(dev, run_loop, kCFRunLoopDefaultMode);
        }
        ctx.response
    }
}

/// Finds all HID devices with the RongYuan vendor collection
pub fn vendor_interfaces() -> Vec<VendorInterface> {
    let mut out = Vec::new();
    unsafe {
        let manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
        if manager.is_null() {
            return out;
        }
        let matching = CFDictionary::from_CFType_pairs(&[(
            CFString::new("VendorID").as_CFType(),
            CFNumber::from(VID_RONGYUAN as i32).as_CFType(),
        )]);
        IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef());
        let set = IOHIDManagerCopyDevices(manager);
        if !set.is_null() {
            let count = CFSetGetCount(set);
            let mut devices: Vec<*const c_void> = vec![std::ptr::null(); count.max(0) as usize];
            CFSetGetValues(set, devices.as_mut_ptr());
            for &dev in &devices {
                let device = DeviceRef::retain(dev as IOHIDDeviceRef);
                let has_vendor = device
                    .property("ReportDescriptor")
                    .and_then(|d| d.downcast::<CFData>())
                    .is_some_and(|d| descriptor_has_vendor_collection(d.bytes()));
                if has_vendor {
                    out.push(VendorInterface {
                        product: device.string("Product"),
                        ids: device.string("SerialNumber").and_then(|s| normalize_address(&s)).into_iter().collect(),
                        device,
                    });
                }
            }
            CFRelease(set as _);
        }
        CFRelease(manager as _);
    }
    out
}
