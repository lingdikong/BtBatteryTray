//! Listens to BlueZ D-Bus signals and notifies the refresh thread on connection changes, battery level changes and newly added battery interfaces.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::thread;

use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{MatchRule, message};

/// The listener thread ends with the process
pub struct ConnectionWatcher;

impl ConnectionWatcher {
    /// Sends one `msg` to `notify` on each relevant change (sent from the listener thread)
    pub fn start<T: Clone + Send + 'static>(notify: Sender<T>, msg: T) -> zbus::Result<Self> {
        let conn = Connection::system()?;
        let rule = MatchRule::builder().msg_type(message::Type::Signal).sender("org.bluez")?.build();
        let signals = MessageIterator::for_match_rule(rule, &conn, None)?;
        thread::Builder::new().name("bluez-watch".into()).spawn(move || {
            for signal in signals.flatten() {
                if is_relevant(&signal) && notify.send(msg.clone()).is_err() {
                    return;
                }
            }
        })?;
        Ok(Self)
    }
}

fn is_relevant(signal: &message::Message) -> bool {
    let header = signal.header();
    let body = signal.body();
    match header.member().map(|m| m.as_str()) {
        Some("PropertiesChanged") => {
            let Ok((iface, changed, _)) = body.deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
            else {
                return false;
            };
            match iface.as_str() {
                "org.bluez.Device1" => changed.contains_key("Connected"),
                "org.bluez.Battery1" => true,
                _ => false,
            }
        }
        // BlueZ only exposes the Battery1 interface after the device connects
        Some("InterfacesAdded") => body
            .deserialize::<(OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>)>()
            .is_ok_and(|(_, ifaces)| ifaces.contains_key("org.bluez.Battery1")),
        _ => false,
    }
}
