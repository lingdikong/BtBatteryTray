//! 通过 SetupAPI 枚举已连接的蓝牙设备，读取 Windows 自带的电量属性。
//!
//! Windows 会把 BLE 电池服务 (0x180F) 和 HFP 耳机上报的电量写到
//! `DEVPKEY_Bluetooth_Battery` 上，挂在 `BTHLE\DEV_xxx` / `BTHENUM\DEV_xxx` 设备节点。

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
    SetupDiGetDevicePropertyW,
};
use windows::Win32::Devices::Properties::{
    DEVPROP_TYPE_BOOLEAN, DEVPROP_TYPE_BYTE, DEVPROP_TYPE_STRING_LIST, DEVPROP_TYPE_UINT32, DEVPROPTYPE,
};
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, PCWSTR, w};

const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};
const DEVPKEY_DEVICE_IS_CONNECTED: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29),
    pid: 15,
};
const DEVPKEY_DEVICE_FRIENDLY_NAME: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};
const DEVPKEY_DEVICE_CONTAINER_CATEGORY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x78c34fc8_104a_4aca_9ea4_524d52996e57),
    pid: 90,
};
const DEVPKEY_BLUETOOTH_CLASS_OF_DEVICE: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
    pid: 10,
};
const DEVPKEY_NAME: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9ebac0),
    pid: 10,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    Headphones,
    Gamepad,
    Other,
}

impl DeviceKind {
    /// `DEVPKEY_DeviceContainer_Category` 的取值，如 `Input.Keyboard`、`Audio.Headphone`
    fn from_category(categories: &[String]) -> Option<Self> {
        categories.iter().find_map(|c| {
            let c = c.to_ascii_lowercase();
            Some(match () {
                _ if c.contains("keyboard") => Self::Keyboard,
                _ if c.contains("mouse") || c.contains("touchpad") => Self::Mouse,
                _ if c.contains("headphone") || c.contains("headset") || c.contains("audio") => Self::Headphones,
                _ if c.contains("gaming") || c.contains("gamepad") || c.contains("joystick") => Self::Gamepad,
                _ => return None,
            })
        })
    }

    /// 经典蓝牙 Class of Device：主类 0x05 外设 / 0x04 音视频
    fn from_class_of_device(cod: u32) -> Option<Self> {
        let major = (cod >> 8) & 0x1f;
        let minor = (cod >> 2) & 0x3f;
        match major {
            0x05 if minor & 0x10 != 0 => Some(Self::Keyboard),
            0x05 if minor & 0x20 != 0 => Some(Self::Mouse),
            0x05 if minor & 0x0f == 0x02 || minor & 0x0f == 0x01 => Some(Self::Gamepad),
            0x04 => Some(Self::Headphones),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BtDevice {
    pub name: String,
    pub kind: DeviceKind,
    /// 12 位大写十六进制蓝牙地址，用于和厂商 HID 通道对应
    pub address: String,
    pub battery: Option<u8>,
}

/// 返回所有当前已连接的蓝牙设备（经典蓝牙 + BLE）
pub fn connected_devices() -> Vec<BtDevice> {
    let mut out = Vec::new();
    for enumerator in [w!("BTHENUM"), w!("BTHLE")] {
        // SAFETY: 句柄在 enumerate 内部使用完毕后销毁
        unsafe { enumerate(enumerator, &mut out) };
    }
    out
}

unsafe fn enumerate(enumerator: PCWSTR, out: &mut Vec<BtDevice>) {
    let Ok(set) = (unsafe { SetupDiGetClassDevsW(None, enumerator, None, DIGCF_ALLCLASSES | DIGCF_PRESENT) })
    else {
        return;
    };
    let mut index = 0;
    loop {
        let mut data = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) }.is_err() {
            break;
        }
        index += 1;

        // 只要设备本体节点（BTHENUM\DEV_xxx / BTHLE\DEV_xxx），跳过各个服务子节点
        let Some(id) = (unsafe { instance_id(set, &data) }) else { continue };
        let Some(address) = parse_dev_address(&id) else { continue };
        if unsafe { get_bool(set, &data, &DEVPKEY_DEVICE_IS_CONNECTED) } != Some(true) {
            continue;
        }
        let name = unsafe { get_string(set, &data, &DEVPKEY_DEVICE_FRIENDLY_NAME) }
            .or_else(|| unsafe { get_string(set, &data, &DEVPKEY_NAME) })
            .unwrap_or_else(|| address.clone());
        let battery = unsafe { get_byte(set, &data, &DEVPKEY_BLUETOOTH_BATTERY) };
        let kind = unsafe { get_string_list(set, &data, &DEVPKEY_DEVICE_CONTAINER_CATEGORY) }
            .and_then(|c| DeviceKind::from_category(&c))
            .or_else(|| unsafe { get_u32(set, &data, &DEVPKEY_BLUETOOTH_CLASS_OF_DEVICE) }.and_then(DeviceKind::from_class_of_device))
            .unwrap_or(DeviceKind::Other);
        out.push(BtDevice { name, kind, address, battery });
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
}

/// `BTHENUM\DEV_B4EE252E439F\...` / `BTHLE\DEV_C57858547FE5\...` → 地址
fn parse_dev_address(instance_id: &str) -> Option<String> {
    let rest = instance_id.split('\\').nth(1)?;
    let addr = rest.strip_prefix("DEV_")?;
    (addr.len() == 12 && addr.chars().all(|c| c.is_ascii_hexdigit())).then(|| addr.to_ascii_uppercase())
}

unsafe fn instance_id(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut len = 0u32;
    unsafe { SetupDiGetDeviceInstanceIdW(set, data, Some(&mut buf), Some(&mut len)) }.ok()?;
    Some(String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize]))
}

unsafe fn get_raw(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<(DEVPROPTYPE, Vec<u8>)> {
    let mut ty = DEVPROPTYPE::default();
    let mut buf = vec![0u8; 512];
    let mut len = 0u32;
    unsafe { SetupDiGetDevicePropertyW(set, data, key, &mut ty, Some(&mut buf), Some(&mut len), 0) }.ok()?;
    buf.truncate(len as usize);
    Some((ty, buf))
}

unsafe fn get_byte(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<u8> {
    match unsafe { get_raw(set, data, key) }? {
        (ty, v) if ty == DEVPROP_TYPE_BYTE && !v.is_empty() => Some(v[0].min(100)),
        _ => None,
    }
}

unsafe fn get_bool(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<bool> {
    match unsafe { get_raw(set, data, key) }? {
        (ty, v) if ty == DEVPROP_TYPE_BOOLEAN && !v.is_empty() => Some(v[0] != 0),
        _ => None,
    }
}

unsafe fn get_string(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
    let (_, v) = unsafe { get_raw(set, data, key) }?;
    let wide: Vec<u16> = v.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let s = String::from_utf16_lossy(&wide).trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

unsafe fn get_u32(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<u32> {
    match unsafe { get_raw(set, data, key) }? {
        (ty, v) if ty == DEVPROP_TYPE_UINT32 && v.len() >= 4 => Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        _ => None,
    }
}

unsafe fn get_string_list(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<Vec<String>> {
    let (ty, v) = unsafe { get_raw(set, data, key) }?;
    if ty != DEVPROP_TYPE_STRING_LIST {
        return None;
    }
    let wide: Vec<u16> = v.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some(wide.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect())
}
