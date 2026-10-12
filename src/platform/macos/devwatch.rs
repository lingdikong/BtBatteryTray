//! Watches Bluetooth HID devices (keyboards, mice, gamepads, etc.) connecting / disconnecting.
//!
//! Uses IOHIDManager's device matching / removal callbacks, matching only devices whose Transport is Bluetooth; devices aren't opened, so no permission is needed.
//! Non-HID devices like AirPods produce no events and rely on the periodic refresh.

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::thread;

use core_foundation::array::CFArray;
use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::{CFRunLoopGetCurrent, CFRunLoopRun, kCFRunLoopDefaultMode};
use core_foundation::string::CFString;

use super::hid::ffi::*;

/// The listener thread ends with the process
pub struct ConnectionWatcher;

type Notify = Box<dyn Fn() + Send>;

unsafe extern "C" fn on_device(context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _device: IOHIDDeviceRef) {
    // SAFETY: context is the Notify leaked in start and never freed
    let notify = unsafe { &*(context as *const Notify) };
    notify();
}

impl ConnectionWatcher {
    /// Sends one `msg` to `notify` on each connection change (each already-connected device is reported once at startup)
    pub fn start<T: Clone + Send + 'static>(notify: Sender<T>, msg: T) -> std::io::Result<Self> {
        thread::Builder::new().name("hid-watch".into()).spawn(move || unsafe {
            let manager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
            if manager.is_null() {
                return;
            }
            let matches: Vec<_> = ["Bluetooth", "Bluetooth Low Energy"]
                .iter()
                .map(|t| CFDictionary::from_CFType_pairs(&[(CFString::new("Transport"), CFString::new(t))]))
                .collect();
            let matches = CFArray::from_CFTypes(&matches);
            IOHIDManagerSetDeviceMatchingMultiple(manager, matches.as_concrete_TypeRef());

            let callback: Notify = Box::new(move || {
                let _ = notify.send(msg.clone());
            });
            let context = Box::into_raw(Box::new(callback)) as *mut c_void;
            IOHIDManagerRegisterDeviceMatchingCallback(manager, on_device, context);
            IOHIDManagerRegisterDeviceRemovalCallback(manager, on_device, context);
            IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            CFRunLoopRun();
        })?;
        Ok(Self)
    }
}
