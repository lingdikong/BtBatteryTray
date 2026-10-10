//! 监听蓝牙设备连接 / 断开，事件到达时通知刷新线程。
//!
//! 用 WinRT DeviceWatcher 观察“已连接的蓝牙设备”这个集合：设备连上时触发 Added，
//! 断开时触发 Removed。经典蓝牙和 BLE 各需一个 watcher。

use std::sync::mpsc::Sender;

use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher};
use windows::Foundation::TypedEventHandler;

/// 持有 watcher，drop 时停止监听
pub struct ConnectionWatcher {
    watchers: Vec<DeviceWatcher>,
}

impl ConnectionWatcher {
    /// 每次连接状态变化向 `notify` 发送一份 `msg`（在系统线程池线程上发送）
    pub fn start<T: Clone + Send + 'static>(notify: Sender<T>, msg: T) -> windows::core::Result<Self> {
        let selectors = [
            BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)?,
            BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)?,
        ];
        let mut watchers = Vec::new();
        for selector in &selectors {
            let watcher = DeviceInformation::CreateWatcherAqsFilter(selector)?;
            let (tx, m) = (notify.clone(), msg.clone());
            watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(move |_, _| {
                let _ = tx.send(m.clone());
                Ok(())
            }))?;
            let (tx, m) = (notify.clone(), msg.clone());
            watcher.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, _| {
                let _ = tx.send(m.clone());
                Ok(())
            }))?;
            // DeviceWatcher 要求 Added / Removed / Updated 都有订阅才会报告变化，Updated 用不到，给个空处理
            watcher.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(|_, _| Ok(())))?;
            watcher.Start()?;
            watchers.push(watcher);
        }
        Ok(Self { watchers })
    }
}

impl Drop for ConnectionWatcher {
    fn drop(&mut self) {
        for w in &self.watchers {
            let _ = w.Stop();
        }
    }
}
