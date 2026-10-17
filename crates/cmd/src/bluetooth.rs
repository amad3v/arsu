//! Live Bluetooth sync and pairing.
//!
//! Every device takes either role, one session at a time, so that any two
//! of them sync, whatever they are: phones only, computers only, or a mix.
//!
//! * **Peripheral** (Linux, and Android phones that can advertise): the
//!   device waits. [`start_sync`] and [`start_pairing`] start a background
//!   task that advertises and serves live sessions
//!   ([`AppState::run_live_session`]) over the platform's [`Peripheral`],
//!   reporting each one's outcome to the interface as a
//!   [`SYNC_SESSION_EVENT`]. The task lives in the managed [`Bluetooth`]
//!   state until it ends or the sync screen stops it; stopping it ends
//!   the session, which stops advertising.
//! * **Central** (Linux and Android): the device finds and connects.
//!   [`find_devices`], [`sync_with`] and [`join`] run over a platform's
//!   [`Central`] — `BlueZ` on Linux, `tauri-plugin-blec` on Android.
//!
//! Both are the same everywhere above the platform's trait: a computer
//! and a phone advertise, serve and connect alike.

use std::{
  sync::{Mutex, PoisonError},
  time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, async_runtime::JoinHandle};
use vault_sync::{
  file::FileError,
  gatt::Advertising,
  invitation::{self, Invitation, TOKEN_LEN, advertisement_token},
};

use crate::{
  AppState,
  clock::unix_now,
  diagnostics::{self, MaskedAddress},
  dto::{NearbyDevice, SyncSummary},
  error::{AppError, AppErrorKind},
  state::{ChunkPeer, LiveRole, labels},
};

/// The event a live session's outcome is reported with.
pub const SYNC_SESSION_EVENT: &str = "sync-session";

/// How long a scan for nearby devices lasts.
pub const SCAN_TIME: Duration = Duration::from_secs(5);

/// How long to look for the device showing an invitation.
pub const PAIRING_SCAN_TIME: Duration = Duration::from_secs(10);

/// How long a session waits for the peer's next chunk before taking it as
/// gone: neither side always hears that the other left.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Waits for a peer's chunks: without end until the session has started
/// (a computer waits for a device for as long as its sync screen asks),
/// then at most [`IDLE_TIMEOUT`] for each.
#[derive(Debug)]
pub(crate) struct Idle {
  started: bool,
}

impl Idle {
  /// For the peripheral, before any device has written.
  pub(crate) fn waiting() -> Self {
    Self { started: false }
  }

  /// For the central, which has connected to start a session.
  pub(crate) fn started() -> Self {
    Self { started: true }
  }

  /// The peer's next chunk; `None` once it is gone, or silent for too long
  /// in a session.
  pub(crate) async fn next<T>(&mut self, chunk: impl Future<Output = Option<T>>) -> Option<T> {
    let next = if self.started {
      tokio::time::timeout(IDLE_TIMEOUT, chunk)
        .await
        .ok()
        .flatten()
    } else {
      chunk.await
    };
    self.started = true;
    next
  }
}

/// A live session's outcome, as the interface receives it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionReport {
  Finished { summary: SyncSummary },
  Failed { error: ErrorReport },
}

/// An error as it crosses IPC (see [`crate::error`]): its kind and its
/// message. Unlike [`AppError`], it can be cloned, as events require.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorReport {
  pub kind: AppErrorKind,
  pub message: String,
}

impl SessionReport {
  #[must_use]
  pub fn of(outcome: Result<SyncSummary, AppError>) -> Self {
    match outcome {
      Ok(summary) => Self::Finished { summary },
      Err(error) => Self::Failed {
        error: ErrorReport {
          kind: error.kind(),
          message: error.message(),
        },
      },
    }
  }
}

/// The running Bluetooth task, if any, and on Linux Arsu's GATT service,
/// kept from its first use to the app's end (see [`ble_linux::GattServer`];
/// Android's plugin keeps the phone's itself).
#[derive(Debug, Default)]
pub struct Bluetooth {
  task: Mutex<Option<JoinHandle<()>>>,
  #[cfg(target_os = "linux")]
  gatt_server: linux::GattServerSlot,
}

impl Bluetooth {
  /// Makes `task` the running one, stopping the previous one.
  pub fn run(&self, task: JoinHandle<()>) {
    if let Some(previous) = self.lock().replace(task) {
      previous.abort();
    }
  }

  /// Stops the running task, if any.
  pub fn stop(&self) {
    if let Some(task) = self.lock().take() {
      task.abort();
    }
  }

  fn lock(&self) -> std::sync::MutexGuard<'_, Option<JoinHandle<()>>> {
    // A poisoned lock still holds a valid handle.
    self.task.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// Whether the sync loop should go on serving devices after `outcome`:
/// yes after a sync, or after a failure that belongs to that one device
/// (it broke off, was not paired, failed the handshake); no when every
/// next session would fail the same way (the vault locked, no adapter).
#[must_use]
pub fn serve_next(outcome: &Result<SyncSummary, AppError>) -> bool {
  match outcome {
    Ok(_) => true,
    Err(error) => matches!(
      error,
      AppError::Link(_) | AppError::Disconnected | AppError::UntrustedDevice
    ),
  }
}

// ---- the peripheral ------------------------------------------------------------

/// A platform's Bluetooth peripheral: Arsu's GATT service, advertising for
/// one session at a time.
pub(crate) trait Peripheral {
  /// A session: it advertises until dropped, and serves the first central
  /// that writes until that central disconnects.
  type Session<'p>: ChunkPeer
  where
    Self: 'p;

  /// Starts a session advertising as `advertising`.
  async fn start_session(
    &mut self,
    advertising: Advertising,
  ) -> Result<Self::Session<'_>, AppError>;
}

/// Starts serving paired devices in the background, one after the other,
/// until [`Bluetooth::stop`] or a session fails in a way the next would too.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn start_sync(app: &AppHandle) {
  let task = tauri::async_runtime::spawn({
    let app = app.clone();
    async move {
      let report = |report| emit(&app, report);
      match platform::peripheral(&app).await {
        Ok(mut peripheral) => serve_sync(&app.state::<AppState>(), &mut peripheral, report).await,
        Err(error) => {
          diagnostics::record!("peripheral unavailable", &error, "role" => labels::SYNC_WAITING);
          report(SessionReport::of(Err(error)));
        }
      }
    }
  });
  app.state::<Bluetooth>().run(task);
}

/// Starts waiting in the background for the device that reads `invitation`,
/// and pairs with it.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn start_pairing(app: &AppHandle, invitation: Invitation, expires_at: u64) {
  let task = tauri::async_runtime::spawn({
    let app = app.clone();
    async move {
      let report = |report| emit(&app, report);
      match platform::peripheral(&app).await {
        Ok(mut peripheral) => {
          serve_pairing(
            &app.state::<AppState>(),
            &mut peripheral,
            invitation,
            expires_at,
            report,
          )
          .await;
        }
        Err(error) => {
          diagnostics::record!(
            "peripheral unavailable",
            &error,
            "role" => labels::PAIRING_INVITING,
          );
          report(SessionReport::of(Err(error)));
        }
      }
    }
  });
  app.state::<Bluetooth>().run(task);
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn emit(app: &AppHandle, report: SessionReport) {
  let _ = app.emit(SYNC_SESSION_EVENT, report);
}

/// Serves paired devices one after the other, reporting each session,
/// until one fails in a way the next would too.
pub(crate) async fn serve_sync(
  state: &AppState,
  peripheral: &mut impl Peripheral,
  report: impl Fn(SessionReport),
) {
  loop {
    let outcome = serve_one(
      state,
      peripheral,
      Advertising::Sync,
      LiveRole::SyncResponder,
    )
    .await;
    let next = serve_next(&outcome);
    report(SessionReport::of(outcome));
    if !next {
      break;
    }
  }
}

/// Waits for the device that read `invitation`, pairs with it, and reports
/// the session. Waiting ends when the invitation expires, at `expires_at`
/// (Unix seconds).
pub(crate) async fn serve_pairing(
  state: &AppState,
  peripheral: &mut impl Peripheral,
  invitation: Invitation,
  expires_at: u64,
  report: impl Fn(SessionReport),
) {
  let token = advertisement_token(&invitation.secret);
  let outcome = match unix_now() {
    Ok(now) => tokio::time::timeout(
      Duration::from_secs(expires_at.saturating_sub(now)),
      serve_one(
        state,
        peripheral,
        Advertising::Pairing(token),
        LiveRole::Invite(invitation),
      ),
    )
    .await
    .unwrap_or(Err(AppError::SyncFile(FileError::NoInvitation))),
    Err(error) => Err(error),
  };
  report(SessionReport::of(outcome));
}

/// One session as the peripheral: advertises as `advertising` until a
/// central has run the session to its end, or disconnected.
async fn serve_one(
  state: &AppState,
  peripheral: &mut impl Peripheral,
  advertising: Advertising,
  role: LiveRole,
) -> Result<SyncSummary, AppError> {
  let label = role.label();
  let outcome = async {
    // Advertising lasts as long as the session.
    let mut session = peripheral.start_session(advertising).await?;
    log::info!(role = label; "advertising");
    state.run_live_session(role, &mut session, 0).await
  }
  .await;
  if let Err(error) = &outcome {
    diagnostics::record!("session failed", error, "role" => label);
  }
  outcome
}

// ---- the central ---------------------------------------------------------------

/// An Arsu peripheral a scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundDevice {
  /// How the platform names it, to connect to it.
  pub address: String,
  /// The invitation token it advertises, in pairing mode.
  pub token: Option<[u8; TOKEN_LEN]>,
}

/// A platform's Bluetooth central: scan for Arsu peripherals, connect to
/// one. The rest is [`AppState::run_live_session`].
pub(crate) trait Central {
  type Peer: ChunkPeer;

  /// The Arsu peripherals that advertise within `duration`.
  async fn scan(&self, duration: Duration) -> Result<Vec<FoundDevice>, AppError>;

  /// Connects to `address`: the peer, and the link's ATT MTU.
  async fn connect(&self, address: &str) -> Result<(Self::Peer, usize), AppError>;

  /// Disconnects from the peer.
  async fn disconnect(&self, peer: Self::Peer);
}

/// The Arsu devices nearby.
///
/// # Errors
///
/// As [`Central::scan`].
pub(crate) async fn find_devices(central: &impl Central) -> Result<Vec<NearbyDevice>, AppError> {
  let found = central.scan(SCAN_TIME).await?;
  log::info!(
    devices = found.len(),
    pairing = found.iter().filter(|device| device.token.is_some()).count();
    "scan finished"
  );
  Ok(
    found
      .into_iter()
      .map(|device| NearbyDevice {
        pairing: device.token.is_some(),
        address: device.address,
      })
      .collect(),
  )
}

/// Syncs with the paired device at `address`, as the central.
///
/// # Errors
///
/// [`AppError::NothingPaired`] before connecting if this device is paired
/// with none; otherwise as [`Central::connect`] and
/// [`AppState::run_live_session`].
pub(crate) async fn sync_with(
  state: &AppState,
  central: &impl Central,
  address: &str,
) -> Result<SyncSummary, AppError> {
  if !state.has_paired_devices()? {
    return Err(AppError::NothingPaired);
  }
  run_central(state, central, address, LiveRole::SyncInitiator).await
}

/// Pairs with the device showing an invitation, given as its QR code's
/// text or as the typed code: finds it by the token it advertises.
///
/// # Errors
///
/// [`AppError::Invitation`] for text that is not an invitation (before
/// any scan), [`AppError::InvitingDeviceNotFound`] if no device nearby
/// shows it, or as [`sync_with`].
pub(crate) async fn join(
  state: &AppState,
  central: &impl Central,
  text: &str,
) -> Result<SyncSummary, AppError> {
  let (secret, inviter) = invitation::read(text)?;
  let token = advertisement_token(&secret);
  let found = central.scan(PAIRING_SCAN_TIME).await?;
  log::info!(
    role = labels::PAIRING_JOINING,
    devices = found.len(),
    pairing = found.iter().filter(|device| device.token.is_some()).count();
    "scan finished"
  );
  let inviting = found
    .into_iter()
    .find(|device| device.token == Some(token))
    .ok_or(AppError::InvitingDeviceNotFound)?;
  run_central(
    state,
    central,
    &inviting.address,
    LiveRole::Join { secret, inviter },
  )
  .await
}

async fn run_central(
  state: &AppState,
  central: &impl Central,
  address: &str,
  role: LiveRole,
) -> Result<SyncSummary, AppError> {
  let label = role.label();
  log::info!(role = label, address:% = MaskedAddress(address); "connecting");
  let (mut peer, att_mtu) = central.connect(address).await?;
  log::info!(role = label, address:% = MaskedAddress(address), att_mtu; "connected");
  let outcome = state.run_live_session(role, &mut peer, att_mtu).await;
  // The session's outcome matters more than a failed goodbye.
  central.disconnect(peer).await;
  outcome
}

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "android")]
use android as platform;

/// This platform's [`Central`].
///
/// # Errors
///
/// [`AppError::BluetoothUnavailable`] when Bluetooth is not ready yet.
#[cfg(target_os = "linux")]
#[expect(
  clippy::unnecessary_wraps,
  reason = "the same signature as Android's, where the plugin may not be ready"
)]
pub(crate) fn central() -> Result<linux::BluezCentral, AppError> {
  Ok(linux::BluezCentral)
}

/// This platform's [`Central`].
///
/// # Errors
///
/// [`AppError::BluetoothUnavailable`] when Bluetooth is not ready yet.
#[cfg(target_os = "android")]
pub(crate) fn central() -> Result<android::BlecCentral, AppError> {
  android::BlecCentral::new()
}

#[cfg(test)]
mod tests;
