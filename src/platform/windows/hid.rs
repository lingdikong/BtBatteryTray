//! Finds RongYuan vendor HID collections on Windows and sends/receives reports.
//!
//! Windows splits HID devices by top-level collection; the vendor collection is its own interface, identifiable via HidP_GetCaps.

use std::time::{Duration, Instant};

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_IDW, CM_Get_Parent, CR_SUCCESS, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidD_GetProductString, HidP_GetCaps, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING, ReadFile, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{PCWSTR, PWSTR};

use crate::rongyuan::{REPORT_LEN, VENDOR_USAGE, VENDOR_USAGE_PAGES, VID_RONGYUAN};

pub struct VendorInterface {
    pub product: Option<String>,
    /// Instance IDs (uppercase) of up to 3 parent levels; the Bluetooth address is embedded in them
    pub ids: Vec<String>,
    path: Vec<u16>,
}

impl VendorInterface {
    pub fn open(&self) -> Option<Device> {
        HidHandle::open(&self.path, true).map(Device)
    }
}

pub struct Device(HidHandle);

impl Device {
    /// Sends a report and waits for the first reply that `accept` approves
    pub fn transact(
        &self,
        req: &[u8; REPORT_LEN],
        accept: &dyn Fn(&[u8]) -> bool,
        timeout: Duration,
    ) -> Option<[u8; REPORT_LEN]> {
        self.0.transact(req, accept, timeout)
    }
}

/// Finds all HID interfaces that match the RongYuan vendor collection signature
pub fn vendor_interfaces() -> Vec<VendorInterface> {
    let mut out = Vec::new();
    unsafe {
        let guid = HidD_GetHidGuid();
        let Ok(set) = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE)
        else {
            return out;
        };
        let mut index = 0;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(set, None, &guid, index, &mut iface).is_err() {
                break;
            }
            index += 1;

            let mut needed = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &iface, None, 0, Some(&mut needed), None);
            if needed == 0 {
                continue;
            }
            // Use a u64-aligned buffer to hold the variable-length struct
            let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            let mut devinfo = SP_DEVINFO_DATA {
                cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiGetDeviceInterfaceDetailW(set, &iface, Some(detail), needed, None, Some(&mut devinfo)).is_err() {
                continue;
            }
            let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
            let len = (0..).take_while(|&i| *path_ptr.add(i) != 0).count();
            let mut path: Vec<u16> = std::slice::from_raw_parts(path_ptr, len).to_vec();
            path.push(0);

            // Only query attributes; no read/write access needed, so other programs are not disturbed
            let Some(probe) = HidHandle::open(&path, false) else { continue };
            if probe.is_vendor_collection() {
                out.push(VendorInterface { product: probe.product(), ids: parent_chain(devinfo.DevInst), path });
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

fn parent_chain(mut devinst: u32) -> Vec<String> {
    let mut ids = Vec::new();
    for _ in 0..3 {
        let mut parent = 0u32;
        if unsafe { CM_Get_Parent(&mut parent, devinst, 0) } != CR_SUCCESS {
            break;
        }
        let mut buf = [0u16; 512];
        if unsafe { CM_Get_Device_IDW(parent, &mut buf, 0) } == CR_SUCCESS {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            ids.push(String::from_utf16_lossy(&buf[..len]).to_ascii_uppercase());
        }
        devinst = parent;
    }
    ids
}

struct HidHandle(HANDLE);

impl HidHandle {
    fn open(path: &[u16], rw: bool) -> Option<Self> {
        let (access, flags) = if rw {
            ((GENERIC_READ | GENERIC_WRITE).0, FILE_FLAG_OVERLAPPED)
        } else {
            (0, FILE_FLAGS_AND_ATTRIBUTES(0))
        };
        unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                flags,
                None,
            )
        }
        .ok()
        .map(Self)
    }

    fn is_vendor_collection(&self) -> bool {
        unsafe {
            let mut attrs = HIDD_ATTRIBUTES {
                Size: size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };
            if !HidD_GetAttributes(self.0, &mut attrs) || attrs.VendorID != VID_RONGYUAN {
                return false;
            }
            let mut pp = PHIDP_PREPARSED_DATA::default();
            if !HidD_GetPreparsedData(self.0, &mut pp) {
                return false;
            }
            let mut caps = HIDP_CAPS::default();
            let ok = HidP_GetCaps(pp, &mut caps).is_ok();
            let _ = HidD_FreePreparsedData(pp);
            ok && VENDOR_USAGE_PAGES.contains(&caps.UsagePage)
                && caps.Usage == VENDOR_USAGE
                && caps.OutputReportByteLength as usize == REPORT_LEN
        }
    }

    fn product(&self) -> Option<String> {
        let mut buf = [0u16; 128];
        let ok = unsafe { HidD_GetProductString(self.0, buf.as_mut_ptr().cast(), (buf.len() * 2) as u32) };
        if !ok {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    fn transact(
        &self,
        req: &[u8; REPORT_LEN],
        accept: &dyn Fn(&[u8]) -> bool,
        timeout: Duration,
    ) -> Option<[u8; REPORT_LEN]> {
        let event = unsafe { CreateEventW(None, true, false, PWSTR::null()) }.ok()?;
        let _event_guard = HandleGuard(event);

        // Post the read first so a reply arriving before the write completes is not missed
        let deadline = Instant::now() + timeout;
        let mut resp = [0u8; REPORT_LEN];
        let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
        let mut pending = self.begin_read(&mut resp, &mut ov);
        if !self.write(req) {
            self.cancel(&mut ov, pending);
            return None;
        }
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.cancel(&mut ov, pending);
                return None;
            }
            if pending && unsafe { WaitForSingleObject(event, left.as_millis() as u32) } != WAIT_OBJECT_0 {
                self.cancel(&mut ov, true);
                return None;
            }
            let mut n = 0u32;
            if unsafe { GetOverlappedResult(self.0, &ov, &mut n, false) }.is_err() {
                return None;
            }
            if accept(&resp[..n as usize]) {
                return Some(resp);
            }
            // Some other report (e.g. event 0x66 or another program's reply), keep waiting
            ov = OVERLAPPED { hEvent: event, ..Default::default() };
            pending = self.begin_read(&mut resp, &mut ov);
        }
    }

    /// Returns true if the read is still pending
    fn begin_read(&self, buf: &mut [u8], ov: &mut OVERLAPPED) -> bool {
        match unsafe { ReadFile(self.0, Some(buf), None, Some(ov)) } {
            Ok(()) => false,
            Err(e) => e.code() == windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult(),
        }
    }

    fn write(&self, data: &[u8]) -> bool {
        let event = match unsafe { CreateEventW(None, true, false, PWSTR::null()) } {
            Ok(e) => HandleGuard(e),
            Err(_) => return false,
        };
        let mut ov = OVERLAPPED { hEvent: event.0, ..Default::default() };
        if let Err(e) = unsafe { WriteFile(self.0, Some(data), None, Some(&mut ov)) } {
            if e.code() != windows::Win32::Foundation::ERROR_IO_PENDING.to_hresult() {
                return false;
            }
            if unsafe { WaitForSingleObject(event.0, 1000) } != WAIT_OBJECT_0 {
                self.cancel(&mut ov, true);
                return false;
            }
        }
        let mut n = 0u32;
        unsafe { GetOverlappedResult(self.0, &ov, &mut n, false) }.is_ok()
    }

    fn cancel(&self, ov: &mut OVERLAPPED, pending: bool) {
        if pending {
            unsafe {
                let _ = CancelIoEx(self.0, Some(ov));
                let mut n = 0u32;
                // Wait for the cancel to actually complete, otherwise the kernel may still write after OVERLAPPED / the buffer is freed
                let _ = GetOverlappedResult(self.0, ov, &mut n, true);
            }
        }
    }
}

impl Drop for HidHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
