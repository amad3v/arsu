//! Bluetooth on Android: this device as the central, through
//! `tauri-plugin-blec`, and as the peripheral, through Arsu's own
//! `ble-android`. A phone finds and joins, and, where its
//! Bluetooth can advertise, waits for the others as a computer does.

use std::time::Duration;

use ble_android::BlePeripheralExt;
use tauri::{AppHandle, Wry};
use tauri_plugin_blec::{
  ALLOW_IBEACONS, Handler, OnDisconnectHandler, check_permissions, get_handler,
  models::{AdapterState, ScanFilter, WriteType},
};
use tokio::sync::mpsc;
use vault_sync::gatt::{Advertising, RX_UUID, SERVICE_UUID, TX_UUID, advertises_arsu, token_of};

use super::{Central, FoundDevice, Idle, Peripheral};
use crate::{error::AppError, state::ChunkPeer};

/// The ATT MTU to ask for: the most Android allows (it asks for it itself
/// from Android 14 on).
const REQUESTED_MTU: u16 = 517;

/// How long past its duration a scan may take before it is stopped: the
/// plugin polls for devices, and each poll takes a moment.
const SCAN_GRACE: Duration = Duration::from_secs(5);

/// The highest Android version (API level) where scanning for Bluetooth
/// needs the location permission: Android 11.
const LAST_LOCATION_API: u32 = 30;

/// `tauri-plugin-blec` as the [`Central`].
pub struct BlecCentral(&'static Handler);

impl BlecCentral {
  /// The plugin's handler, once the plugin has started.
  ///
  /// # Errors
  ///
  /// [`AppError::Blec`] before the plugin is ready.
  pub fn new() -> Result<Self, AppError> {
    Ok(Self(get_handler().map_err(AppError::Blec)?))
  }

  /// Checks that Bluetooth is on, before anything that needs it.
  ///
  /// Asked first rather than learnt from a failure: the plugin's scan,
  /// started with Bluetooth off, fails with an error that does not say
  /// so, and keeps refusing every later scan as one already running
  /// (its Android half records the scan before Android refuses it). A
  /// state the plugin cannot tell is left for the operation to find out.
  ///
  /// # Errors
  ///
  /// [`AppError::BluetoothOff`] if Bluetooth is off.
  async fn ensure_on(&self) -> Result<(), AppError> {
    if matches!(self.0.get_adapter_state().await, AdapterState::Off) {
      return Err(AppError::BluetoothOff);
    }
    Ok(())
  }

  /// Readies the connected peripheral for a session: notifications from
  /// `tx` before anything is written, as the peripheral answers by them.
  async fn open(&self) -> Result<BlecPeer, AppError> {
    let (notified, incoming) = mpsc::unbounded_channel();
    self
      .0
      .subscribe(TX_UUID, Some(SERVICE_UUID), move |chunk: Vec<u8>| {
        // The session is gone if nobody receives; the chunk with it.
        let _ = notified.send(chunk);
      })
      .await
      .map_err(AppError::Blec)?;
    let att_mtu = usize::from(self.0.mtu().await.map_err(AppError::Blec)?);
    Ok(BlecPeer {
      handler: self.0,
      incoming,
      idle: Idle::started(),
      att_mtu,
    })
  }
}

impl Central for BlecCentral {
  type Peer = BlecPeer;

  async fn scan(&self, duration: Duration) -> Result<Vec<FoundDevice>, AppError> {
    self.ensure_on().await?;
    let needs_location = needs_location_permission();
    // The plugin asks for location below Android 12 only with this flag
    // (its "iBeacon" support); from Android 12 on, asking would fail, as
    // the manifest stops declaring location there.
    ALLOW_IBEACONS.store(needs_location, std::sync::atomic::Ordering::Release);
    if !check_permissions(true).await.map_err(AppError::Blec)? {
      log::info!(location_asked = needs_location; "scan permission refused");
      return Err(AppError::BluetoothPermissionDenied);
    }

    let (updates, mut found) = mpsc::channel(8);
    let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    self
      .0
      // Unfiltered: the plugin's service filter misses a computer waiting
      // to pair, which advertises only data for the service. Arsu's devices are picked out below.
      .discover(Some(updates), millis, ScanFilter::None, needs_location)
      .await
      .map_err(AppError::Blec)?;
    // Each update is the whole list so far, and the plugin closes the
    // channel once the scan ends: the last update is the answer.
    let mut latest = Vec::new();
    let deadline = tokio::time::sleep(duration + SCAN_GRACE);
    tokio::pin!(deadline);
    loop {
      tokio::select! {
        update = found.recv() => match update {
          Some(devices) => latest = devices,
          None => break,
        },
        () = &mut deadline => {
          // Stop the scan before dropping the channel: the plugin's scan
          // panics on sending to a closed one, and would never stop.
          let _ = self.0.stop_scan().await;
          break;
        }
      }
    }
    log::info!(devices_seen = latest.len(), location_asked = needs_location; "scan ended");
    Ok(
      latest
        .into_iter()
        .filter(|device| advertises_arsu(&device.services, &device.service_data))
        .map(|device| FoundDevice {
          token: token_of(&device.service_data),
          address: device.address,
        })
        .collect(),
    )
  }

  async fn connect(&self, address: &str) -> Result<(BlecPeer, usize), AppError> {
    self.ensure_on().await?;
    Handler::set_android_mtu_request(REQUESTED_MTU);
    // The plugin scans again for an address it has not seen; that scan
    // needs location as much as the first one.
    self
      .0
      .connect(
        address,
        OnDisconnectHandler::None,
        needs_location_permission(),
      )
      .await
      .map_err(AppError::Blec)?;
    match self.open().await {
      Ok(peer) => {
        let att_mtu = peer.att_mtu;
        Ok((peer, att_mtu))
      }
      Err(error) => {
        // Connected but unusable: leave nothing open behind.
        let _ = self.0.disconnect().await;
        Err(error)
      }
    }
  }

  async fn disconnect(&self, _peer: BlecPeer) {
    let _ = self.0.disconnect().await;
  }
}

/// A connection to a peripheral, as a [`ChunkPeer`].
pub struct BlecPeer {
  handler: &'static Handler,
  incoming: mpsc::UnboundedReceiver<Vec<u8>>,
  idle: Idle,
  att_mtu: usize,
}

impl ChunkPeer for BlecPeer {
  async fn next_chunk(&mut self) -> Option<(Vec<u8>, usize)> {
    let chunk = self.idle.next(self.incoming.recv()).await?;
    Some((chunk, self.att_mtu))
  }

  async fn send_chunk(&mut self, chunk: Vec<u8>) -> Result<(), AppError> {
    self
      .handler
      .send_data(RX_UUID, Some(SERVICE_UUID), &chunk, WriteType::WithResponse)
      .await
      .map_err(AppError::Blec)
  }
}

/// Whether this Android version ties Bluetooth scanning to the location
/// permission (Android 11 and lower).
fn needs_location_permission() -> bool {
  android_system_properties::AndroidSystemProperties::new()
    .get("ro.build.version.sdk")
    .and_then(|level| level.parse::<u32>().ok())
    .is_some_and(|level| level <= LAST_LOCATION_API)
}

// ---- the peripheral ------------------------------------------------------------

/// This phone's [`Peripheral`], once it may advertise and serve.
///
/// # Errors
///
/// [`AppError::BluetoothPermissionDenied`] if the user refuses the
/// Nearby devices permission (Android 12+), or as
/// [`peripheral_error`].
pub(crate) async fn peripheral(app: &AppHandle) -> Result<PhonePeripheral, AppError> {
  let plugin = app.ble_peripheral().clone();
  plugin.ensure_permission().await.map_err(peripheral_error)?;
  Ok(PhonePeripheral(plugin))
}

/// `ble-android` as the [`Peripheral`].
pub struct PhonePeripheral(ble_android::BlePeripheral<Wry>);

impl Peripheral for PhonePeripheral {
  type Session<'p> = PhoneSession;

  async fn start_session(&mut self, advertising: Advertising) -> Result<PhoneSession, AppError> {
    let session = self.0.start(advertising).await.map_err(peripheral_error)?;
    Ok(PhoneSession {
      session,
      idle: Idle::waiting(),
    })
  }
}

/// The phone's peripheral session, as a [`ChunkPeer`]: it advertises for
/// as long as it lives.
pub struct PhoneSession {
  session: ble_android::Session<Wry>,
  idle: Idle,
}

impl ChunkPeer for PhoneSession {
  async fn next_chunk(&mut self) -> Option<(Vec<u8>, usize)> {
    let (chunk, mtu) = self.idle.next(self.session.next_chunk()).await?;
    Some((chunk, usize::from(mtu)))
  }

  async fn send_chunk(&mut self, chunk: Vec<u8>) -> Result<(), AppError> {
    self.session.notify(&chunk).await.map_err(peripheral_error)
  }
}

/// What the peripheral's failure means to the user. A plain failure keeps
/// its details for the diagnostic log ([`AppError::hidden_cause`]).
fn peripheral_error(error: ble_android::Error) -> AppError {
  match error {
    ble_android::Error::Off => AppError::BluetoothOff,
    ble_android::Error::Unsupported => AppError::CannotWait,
    ble_android::Error::PermissionDenied => AppError::BluetoothPermissionDenied,
    ble_android::Error::Failed(_) => AppError::Peripheral(error),
  }
}
