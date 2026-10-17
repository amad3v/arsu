//! This device as the BLE central (computer to computer): find
//! Arsu peripherals, connect to one, and move chunks.

use std::{pin::Pin, time::Duration};

use bluer::{AdapterEvent, Address, Device, gatt::remote::Characteristic};
use futures_util::{Stream, StreamExt};
use vault_sync::{
  gatt::{RX_UUID, SERVICE_UUID, TX_UUID, advertises_arsu, token_of},
  invitation::TOKEN_LEN,
};

use crate::BleError;

/// An Arsu peripheral found nearby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
  pub address: Address,
  /// The invitation token it advertises, in pairing mode.
  pub token: Option<[u8; TOKEN_LEN]>,
}

/// The Arsu peripherals that advertise within `duration`.
///
/// # Errors
///
/// [`BleError::Bluetooth`] if there is no adapter or discovery fails.
pub async fn scan(duration: Duration) -> Result<Vec<Found>, BleError> {
  let session = bluer::Session::new().await?;
  let adapter = session.default_adapter().await?;
  adapter.set_powered(true).await?;
  let mut events = adapter.discover_devices().await?;
  let mut seen: Vec<Address> = Vec::new();
  let deadline = tokio::time::sleep(duration);
  tokio::pin!(deadline);
  loop {
    tokio::select! {
      () = &mut deadline => break,
      event = events.next() => match event {
        Some(AdapterEvent::DeviceAdded(address)) if !seen.contains(&address) => seen.push(address),
        Some(_) => {}
        None => break,
      },
    }
  }
  // Read what each device advertises once the scan is over: a device
  // `BlueZ` already knew is reported at once, with what it advertised
  // then (a computer that has since started pairing, say).
  let mut found = Vec::new();
  for address in seen {
    let device = adapter.device(address)?;
    let uuids = device.uuids().await?.unwrap_or_default();
    let service_data = device.service_data().await?.unwrap_or_default();
    if advertises_arsu(&uuids, &service_data) {
      found.push(Found {
        address,
        token: token_of(&service_data),
      });
    }
  }
  Ok(found)
}

/// A connection to an Arsu peripheral.
pub struct Central {
  device: Device,
  rx: Characteristic,
  notifications: Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>,
  att_mtu: usize,
}

impl Central {
  /// Connects to the peripheral at `address`, subscribes to its
  /// notifications and learns the link's MTU.
  ///
  /// # Errors
  ///
  /// [`BleError::Bluetooth`] if the connection fails;
  /// [`BleError::NotArsu`] if the device does not serve Arsu's service.
  pub async fn connect(address: Address) -> Result<Self, BleError> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    let device = adapter.device(address)?;
    if !device.is_connected().await? {
      device.connect().await?;
    }
    let (rx, tx) = arsu_characteristics(&device).await?;
    // Subscribe before writing anything: the peripheral answers by
    // notification.
    let notifications = Box::pin(tx.notify().await?);
    // The characteristic's `MTU` property is the link's ATT MTU. (Its
    // write socket would tell too, but `BlueZ` gives one only for
    // characteristics that take writes without response, which Arsu's do
    // not, on a computer or a phone.)
    let att_mtu = rx.mtu().await?;
    Ok(Self {
      device,
      rx,
      notifications,
      att_mtu,
    })
  }

  /// The link's ATT MTU, as far as chunk sizes go.
  #[must_use]
  pub fn att_mtu(&self) -> usize {
    self.att_mtu
  }

  /// The next notified chunk; `None` once the peripheral is gone.
  pub async fn next(&mut self) -> Option<Vec<u8>> {
    self.notifications.next().await
  }

  /// Writes a chunk, with response.
  ///
  /// # Errors
  ///
  /// [`BleError::Bluetooth`] if the write fails.
  pub async fn send(&mut self, chunk: &[u8]) -> Result<(), BleError> {
    Ok(self.rx.write(chunk).await?)
  }

  /// Disconnects.
  ///
  /// # Errors
  ///
  /// [`BleError::Bluetooth`] if `BlueZ` refuses.
  pub async fn disconnect(self) -> Result<(), BleError> {
    Ok(self.device.disconnect().await?)
  }
}

/// The `rx` and `tx` characteristics of Arsu's service on `device`.
async fn arsu_characteristics(
  device: &Device,
) -> Result<(Characteristic, Characteristic), BleError> {
  for service in device.services().await? {
    if service.uuid().await? != SERVICE_UUID {
      continue;
    }
    let (mut rx, mut tx) = (None, None);
    for characteristic in service.characteristics().await? {
      match characteristic.uuid().await? {
        uuid if uuid == RX_UUID => rx = Some(characteristic),
        uuid if uuid == TX_UUID => tx = Some(characteristic),
        _ => {}
      }
    }
    if let (Some(rx), Some(tx)) = (rx, tx) {
      return Ok((rx, tx));
    }
  }
  Err(BleError::NotArsu)
}
