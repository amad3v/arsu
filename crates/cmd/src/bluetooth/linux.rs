//! Bluetooth on Linux, through `BlueZ`: this device as the peripheral
//! (serving sync and pairing) and as the central (to any other device).

use std::sync::Arc;

use ble_linux::{Address, Advertised, GattServer, scan};
use tauri::{AppHandle, Manager};
use tokio::sync::OwnedMutexGuard;
use vault_sync::gatt::Advertising;

use super::{Bluetooth, Central, FoundDevice, Idle, Peripheral};
use crate::{error::AppError, state::ChunkPeer};

/// Arsu's GATT service, registered by the first session that needs it.
/// A serving task holds the lock for as long as it serves; stopping the
/// task releases it, and the next task finds the same service.
pub type GattServerSlot = Arc<tokio::sync::Mutex<Option<GattServer>>>;

/// This computer's [`Peripheral`], once the previous serving task has let
/// go of it. Never fails here (registering the service waits for the
/// first session, whose failure is reported like any other); Android's
/// asks for a permission first.
///
/// # Errors
///
/// None on Linux.
pub(crate) async fn peripheral(app: &AppHandle) -> Result<BluezPeripheral, AppError> {
  let slot = Arc::clone(&app.state::<Bluetooth>().gatt_server);
  Ok(BluezPeripheral(slot.lock_owned().await))
}

/// `BlueZ` as the [`Peripheral`]: the slot of Arsu's GATT service.
pub struct BluezPeripheral(OwnedMutexGuard<Option<GattServer>>);

impl Peripheral for BluezPeripheral {
  type Session<'p> = PeripheralPeer<'p>;

  async fn start_session(
    &mut self,
    advertising: Advertising,
  ) -> Result<PeripheralPeer<'_>, AppError> {
    let server = match self.0.take() {
      Some(server) => server,
      None => GattServer::register()
        .await
        .inspect(|_| log::info!("gatt service registered"))
        .map_err(AppError::Bluetooth)?,
    };
    let server = self.0.insert(server);
    let advertised = server
      .start_session(advertising)
      .await
      .map_err(AppError::Bluetooth)?;
    Ok(PeripheralPeer {
      server,
      idle: Idle::waiting(),
      _advertised: advertised,
    })
  }
}

/// `BlueZ` as the [`Central`].
pub struct BluezCentral;

impl Central for BluezCentral {
  type Peer = CentralPeer;

  async fn scan(&self, duration: std::time::Duration) -> Result<Vec<FoundDevice>, AppError> {
    let found = scan(duration).await.map_err(AppError::Bluetooth)?;
    Ok(
      found
        .into_iter()
        .map(|device| FoundDevice {
          address: device.address.to_string(),
          token: device.token,
        })
        .collect(),
    )
  }

  async fn connect(&self, address: &str) -> Result<(CentralPeer, usize), AppError> {
    let address: Address = address
      .parse()
      .map_err(|_| AppError::InvalidDeviceId(address.to_owned()))?;
    let central = ble_linux::Central::connect(address)
      .await
      .map_err(AppError::Bluetooth)?;
    let att_mtu = central.att_mtu();
    Ok((
      CentralPeer {
        central,
        idle: Idle::started(),
      },
      att_mtu,
    ))
  }

  async fn disconnect(&self, peer: CentralPeer) {
    let _ = peer.central.disconnect().await;
  }
}

/// A `BlueZ` connection to a peripheral, as a [`ChunkPeer`].
pub struct CentralPeer {
  central: ble_linux::Central,
  idle: Idle,
}

impl ChunkPeer for CentralPeer {
  async fn next_chunk(&mut self) -> Option<(Vec<u8>, usize)> {
    let mtu = self.central.att_mtu();
    let chunk = self.idle.next(self.central.next()).await?;
    Some((chunk, mtu))
  }

  async fn send_chunk(&mut self, chunk: Vec<u8>) -> Result<(), AppError> {
    self.central.send(&chunk).await.map_err(AppError::Bluetooth)
  }
}

/// The `BlueZ` peripheral's session, as a [`ChunkPeer`]: it advertises
/// for as long as it lives.
pub struct PeripheralPeer<'server> {
  server: &'server mut GattServer,
  idle: Idle,
  _advertised: Advertised,
}

impl ChunkPeer for PeripheralPeer<'_> {
  async fn next_chunk(&mut self) -> Option<(Vec<u8>, usize)> {
    let (chunk, mtu) = self.idle.next(self.server.next_chunk()).await?;
    Some((chunk, usize::from(mtu)))
  }

  async fn send_chunk(&mut self, chunk: Vec<u8>) -> Result<(), AppError> {
    self.server.send(chunk).await.map_err(AppError::Bluetooth)
  }
}
