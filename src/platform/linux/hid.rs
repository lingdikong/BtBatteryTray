//! On Linux, finds the RongYuan vendor collection and exchanges reports via hidraw.
//!
//! A hidraw node covers the whole HID device (unlike Windows, which splits by collection), so the report descriptor must be read to know whether it has the vendor collection.
//! Regular users have no read/write access to /dev/hidraw* by default; a udev rule must be installed (see packaging/linux).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::bluetooth::normalize_address;
use crate::rongyuan::{REPORT_LEN, VID_RONGYUAN, descriptor_has_vendor_collection};

pub struct VendorInterface {
    pub product: Option<String>,
    /// Device Bluetooth address (HID_UNIQ)
    pub ids: Vec<String>,
    path: PathBuf,
}

impl VendorInterface {
    pub fn open(&self) -> Option<Device> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&self.path)
            .ok()
            .map(Device)
    }
}

pub struct Device(File);

impl Device {
    /// Sends a report and waits for the first reply that `accept` approves
    pub fn transact(
        &self,
        req: &[u8; REPORT_LEN],
        accept: &dyn Fn(&[u8]) -> bool,
        timeout: Duration,
    ) -> Option<[u8; REPORT_LEN]> {
        let mut file = &self.0;
        // Numbered report: the first byte is the Report ID
        file.write_all(req).ok()?;
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 256];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let mut fds = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: fds is a valid pollfd
            if unsafe { libc::poll(&mut fds, 1, left.as_millis().max(1) as libc::c_int) } <= 0 {
                return None;
            }
            let n = match file.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(_) => return None,
            };
            // Other reports (keystrokes, event 0x66, etc.): keep waiting
            if accept(&buf[..n]) {
                let mut resp = [0u8; REPORT_LEN];
                let len = n.min(REPORT_LEN);
                resp[..len].copy_from_slice(&buf[..len]);
                return Some(resp);
            }
        }
    }
}

/// Finds all hidraw devices with the RongYuan vendor collection
pub fn vendor_interfaces() -> Vec<VendorInterface> {
    let Ok(dir) = fs::read_dir("/sys/class/hidraw") else { return Vec::new() };
    dir.flatten()
        .filter_map(|entry| {
            let sys = entry.path().join("device");
            let uevent = fs::read_to_string(sys.join("uevent")).ok()?;
            let field = |key: &str| uevent.lines().find_map(|l| l.strip_prefix(key)).map(str::trim);
            // HID_ID=0005:00003151:00004012 → bus:VID:PID
            let vid = field("HID_ID=")?.split(':').nth(1).and_then(|v| u32::from_str_radix(v, 16).ok())?;
            if vid != VID_RONGYUAN as u32 {
                return None;
            }
            let desc = fs::read(sys.join("report_descriptor")).ok()?;
            if !descriptor_has_vendor_collection(&desc) {
                return None;
            }
            Some(VendorInterface {
                product: field("HID_NAME=").filter(|s| !s.is_empty()).map(String::from),
                ids: field("HID_UNIQ=").and_then(normalize_address).into_iter().collect(),
                path: PathBuf::from("/dev").join(entry.file_name()),
            })
        })
        .collect()
}
