//! 荣源 (RongYuan, VID 0x3151) 方案蓝牙键盘的私有电量协议。
//!
//! 腹灵 NX87、Akko、MonsGeek、Epomaker 等大量键盘用这套固件。经典蓝牙/BLE 连接时，
//! 键盘会暴露一个厂商 HID 集合（UsagePage 0xFF66 / 0xFF55，Usage 0x0202，Report ID 6），
//! 收发 66 字节报告：
//!
//! ```text
//! 请求: [06] [55] [cmd] [d1..d6] [checksum] [0...]   checksum = 0xFF - (cmd+d1..d6)
//! 应答: [06] [55] [cmd] [r1] [r2] ...
//! ```
//!
//! `GET_BATTERY (0x83)` 应答中 r1 = 电量百分比，r2 = 0 未充电 / 1 充电中 / 2 已充满。
//! 协议参考 github.com/echtzeit-solutions/monsgeek-akko-linux 和官方驱动前端代码。

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

const VID_RONGYUAN: u16 = 0x3151;
const VENDOR_USAGE_PAGES: [u16; 2] = [0xFF66, 0xFF55];
const VENDOR_USAGE: u16 = 0x0202;
const REPORT_ID: u8 = 0x06;
const CMD_MARKER: u8 = 0x55;
const CMD_GET_BATTERY: u8 = 0x83;
const REPORT_LEN: usize = 66;

#[derive(Debug, Clone)]
pub struct VendorBattery {
    pub product: String,
    /// 设备树上能找到的蓝牙地址（12 位大写十六进制），用来和系统蓝牙设备列表对应
    pub parent_ids: Vec<String>,
    pub level: u8,
    pub charging: bool,
}

impl VendorBattery {
    pub fn matches_address(&self, address: &str) -> bool {
        self.parent_ids.iter().any(|id| id.contains(address))
    }
}

/// 查询所有荣源方案蓝牙设备的电量
pub fn query_all() -> Vec<VendorBattery> {
    vendor_interfaces()
        .into_iter()
        .filter_map(|(path, parent_ids)| query(&path, parent_ids))
        .collect()
}

fn query(path: &[u16], parent_ids: Vec<String>) -> Option<VendorBattery> {
    let dev = HidHandle::open(path, true)?;
    let product = dev.product().unwrap_or_else(|| "RongYuan keyboard".into());
    // 键盘刚唤醒或忙时偶尔不应答，再试一次
    let resp = dev
        .transact(CMD_GET_BATTERY, Duration::from_millis(800))
        .or_else(|| dev.transact(CMD_GET_BATTERY, Duration::from_millis(1500)))?;
    Some(VendorBattery {
        product,
        parent_ids,
        level: resp[3].min(100),
        charging: resp[4] == 1,
    })
}

/// 找出所有符合荣源厂商集合特征的 HID 接口，返回 (设备路径, 父节点实例 ID 链)
fn vendor_interfaces() -> Vec<(Vec<u16>, Vec<String>)> {
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
            // 用 u64 对齐的缓冲区承载变长结构体
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

            if is_vendor_collection(&path) {
                out.push((path, parent_chain(devinfo.DevInst)));
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

fn is_vendor_collection(path: &[u16]) -> bool {
    // 只取属性，不需要读写权限，不会打扰其他程序
    let Some(dev) = HidHandle::open(path, false) else { return false };
    unsafe {
        let mut attrs = HIDD_ATTRIBUTES {
            Size: size_of::<HIDD_ATTRIBUTES>() as u32,
            ..Default::default()
        };
        if !HidD_GetAttributes(dev.0, &mut attrs) || attrs.VendorID != VID_RONGYUAN {
            return false;
        }
        let mut pp = PHIDP_PREPARSED_DATA::default();
        if !HidD_GetPreparsedData(dev.0, &mut pp) {
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

/// 向上取 3 层父节点的实例 ID（大写），蓝牙地址就藏在其中
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

    /// 发送一条无参数命令，等待回显同一命令字节的应答
    fn transact(&self, cmd: u8, timeout: Duration) -> Option<[u8; REPORT_LEN]> {
        let mut req = [0u8; REPORT_LEN];
        req[0] = REPORT_ID;
        req[1] = CMD_MARKER;
        req[2] = cmd;
        req[9] = 0xFF - (req[2..9].iter().map(|&b| b as u32).sum::<u32>() & 0xFF) as u8;

        let event = unsafe { CreateEventW(None, true, false, PWSTR::null()) }.ok()?;
        let _event_guard = HandleGuard(event);

        // 先挂起读，避免应答在写完之前就到达而错过
        let deadline = Instant::now() + timeout;
        let mut resp = [0u8; REPORT_LEN];
        let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
        let mut pending = self.begin_read(&mut resp, &mut ov);
        if !self.write(&req) {
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
            if n as usize >= 5 && resp[0] == REPORT_ID && resp[1] == CMD_MARKER && resp[2] == cmd {
                return Some(resp);
            }
            // 别的报告（比如事件 0x66 或其他程序的应答），继续等
            ov = OVERLAPPED { hEvent: event, ..Default::default() };
            pending = self.begin_read(&mut resp, &mut ov);
        }
    }

    /// 返回 true 表示读操作仍在进行中
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
                // 等取消真正完成，否则 OVERLAPPED / 缓冲区释放后内核还可能写入
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
