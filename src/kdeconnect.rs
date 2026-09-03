// SPDX-License-Identifier: GPL-3.0-only

//! Dialling and texting through a paired phone.
//!
//! A desktop address book that shows a phone number and cannot do anything
//! with it is a lookup table. `tel:` already goes to whatever the desktop
//! registered — which, when KDE Connect is installed, is usually KDE Connect.
//! This module is for the two things that path cannot do:
//!
//! - **Say whether it will work.** A `tel:` handed to `xdg-open` with no
//!   handler fails silently. Asking the daemon for a reachable device first
//!   means the button is only offered when pressing it does something.
//! - **Send an SMS.** `sms:` is registered far less consistently than `tel:`,
//!   and the daemon's own method takes a message body, which a URI handler
//!   does not.
//!
//! Placing a call is deliberately **not** here. KDE Connect's desktop-to-phone
//! API sends messages; it does not dial. The `tel:` button already reaches
//! `kdeconnect-handler` through the desktop's own URI registration, which is
//! the supported path, and a "Call" button here would either do nothing or —
//! worse, via the SMS method with an empty body — text the person a blank
//! message.
//!
//! Everything here degrades to `None`: no daemon, no paired device, no phone
//! in reach — the caller falls back to the desktop handler and the number
//! stays copyable regardless. Nothing in Circle *requires* KDE Connect.

/// A phone that is paired and currently reachable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

/// The daemon's bus name and object path.
const SERVICE: &str = "org.kde.kdeconnect";
const DAEMON_PATH: &str = "/modules/kdeconnect";

/// Every paired, reachable device, or an empty list when the daemon is not
/// running at all.
///
/// Deliberately not an error type: "KDE Connect is not installed" is the
/// common case on a COSMIC desktop and is not a failure of anything.
pub async fn devices() -> Vec<Device> {
    match query_devices().await {
        Ok(devices) => devices,
        Err(why) => {
            // Debug, not warn: on a machine without KDE Connect this fires on
            // every launch and means nothing is wrong.
            tracing::debug!(%why, "no KDE Connect devices");
            Vec::new()
        }
    }
}

async fn query_devices() -> zbus::Result<Vec<Device>> {
    let connection = zbus::Connection::session().await?;
    let daemon = zbus::Proxy::new(
        &connection,
        SERVICE,
        DAEMON_PATH,
        "org.kde.kdeconnect.daemon",
    )
    .await?;

    // `onlyReachable` and `onlyPaired` both true: a device that is paired but
    // out of range would give the user a button that silently does nothing,
    // which is the failure mode this module exists to avoid.
    let ids: Vec<String> = daemon.call("devices", &(true, true)).await?;

    let mut devices = Vec::with_capacity(ids.len());
    for id in ids {
        let name = device_name(&connection, &id)
            .await
            .unwrap_or_else(|_| id.clone());
        devices.push(Device { id, name });
    }
    Ok(devices)
}

async fn device_name(connection: &zbus::Connection, id: &str) -> zbus::Result<String> {
    let device = zbus::Proxy::new(
        connection,
        SERVICE,
        device_path(id),
        "org.kde.kdeconnect.device",
    )
    .await?;
    device.get_property("name").await
}

/// Asks a device to text a number.
///
/// An empty body is refused rather than sent: the SMS method accepts one, and
/// a blank text to somebody's phone is not a thing any button should be able
/// to do by accident.
pub async fn send_sms(device: &str, number: &str, body: &str) -> Result<(), String> {
    if body.trim().is_empty() {
        return Err(String::from("refusing to send an empty message"));
    }

    let connection = zbus::Connection::session()
        .await
        .map_err(|why| why.to_string())?;
    let proxy = zbus::Proxy::new(
        &connection,
        SERVICE,
        format!("{}/sms", device_path(device)),
        "org.kde.kdeconnect.device.sms",
    )
    .await
    .map_err(|why| why.to_string())?;

    proxy
        .call::<_, _, ()>("sendSms", &(number.to_owned(), body.to_owned()))
        .await
        .map_err(|why| why.to_string())
}

/// A device id's object path.
///
/// KDE Connect derives it by replacing everything that is not alphanumeric
/// with an underscore — a device id is a certificate fingerprint or a MAC-like
/// string, and the raw value is not a legal D-Bus path element.
#[must_use]
pub fn device_path(id: &str) -> String {
    let sanitised: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{DAEMON_PATH}/devices/{sanitised}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dash in a device id is legal there and illegal in a D-Bus path; the
    /// call fails with an obscure error if this is wrong.
    #[test]
    fn a_device_path_is_a_legal_object_path() {
        let path = device_path("aa11-bb22:cc33");
        assert_eq!(path, "/modules/kdeconnect/devices/aa11_bb22_cc33");
        assert!(
            path.split('/').skip(1).all(|element| element
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')),
            "{path} is not a legal D-Bus object path"
        );
    }

    /// The whole module has to be absent-friendly: no daemon is the normal
    /// case on a COSMIC desktop, and it must return promptly rather than
    /// hanging the caller or panicking.
    #[tokio::test]
    async fn asking_for_devices_returns_promptly_with_or_without_a_daemon() {
        let answered = tokio::time::timeout(std::time::Duration::from_secs(5), devices()).await;
        assert!(
            answered.is_ok(),
            "the device query hung; a missing daemon must not block the UI"
        );
    }

    /// Nothing should be able to text somebody a blank message.
    #[tokio::test]
    async fn an_empty_message_is_refused_before_any_bus_call() {
        assert!(send_sms("device", "+30 210 1234567", "   ").await.is_err());
    }
}
