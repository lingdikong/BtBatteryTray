//! Watches Bluetooth device connect / disconnect and notifies the refresh thread on events.
//!
//! Uses a WinRT DeviceWatcher on the "connected Bluetooth devices" set: Added fires when a device connects,
//! Removed when it disconnects. Classic Bluetooth and BLE each need their own watcher.

use std::sync::mpsc::Sender;

use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher};
use windows::Foundation::TypedEventHandler;

/// Holds the watchers; stops watching on drop
pub struct ConnectionWatcher {
    watchers: Vec<DeviceWatcher>,
}

impl ConnectionWatcher {
    /// Sends one `msg` to `notify` on each connection state change (sent from a system thread pool thread)
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
            // DeviceWatcher only reports changes if Added / Removed / Updated are all subscribed; Updated is unused, so give it an empty handler
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
