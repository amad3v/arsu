//! Arsu as a Bluetooth LE peripheral on Android: the phone
//! advertises and serves Arsu's GATT service the way a computer does
//! (`ble-linux`), so that any device, a phone or a computer, finds and
//! syncs with it the same way. It moves chunks and decides nothing: they
//! are those of a `vault_sync::gatt::GattLink`, which the caller runs.
//!
//! * [`BlePeripheral::start`] starts a [`Session`]: the phone advertises
//!   until the session is dropped, and serves the first central that
//!   writes to `rx`, refusing every other device until the next session.
//! * The central's chunks arrive through [`Session::next_chunk`], which
//!   ends once it disconnects; [`Session::notify`] sends one on `tx`.
//! * The service itself is opened once, by the first session, and kept
//!   for the app's life, so that its attribute handles never change.
//!
//! The Kotlin half (`android/`) talks to Android's Bluetooth stack.
//! Driven from Rust only: the plugin has no commands for the `WebView`.

#![cfg(target_os = "android")]

use std::sync::{
  Arc, Mutex, PoisonError,
  atomic::{AtomicU64, Ordering},
};

use base64::{Engine, prelude::BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use tauri::{
  Manager, Runtime,
  ipc::{Channel, InvokeResponseBody},
  plugin::{Builder, PluginHandle, TauriPlugin, mobile::PluginInvokeError},
};
use tokio::sync::mpsc;
use vault_sync::gatt::Advertising;

const PLUGIN_IDENTIFIER: &str = "io.github.amad3v.arsu.ble";

/// Why the peripheral could not do what was asked.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("Bluetooth is off")]
  Off,
  /// The phone's Bluetooth cannot advertise, which some do not.
  #[error("this phone cannot advertise over Bluetooth")]
  Unsupported,
  #[error("the Nearby devices permission was refused")]
  PermissionDenied,
  #[error("the Bluetooth peripheral failed: {0}")]
  Failed(String),
}

impl From<PluginInvokeError> for Error {
  fn from(error: PluginInvokeError) -> Self {
    match error {
      PluginInvokeError::InvokeRejected(response) => match response.code.as_deref() {
        Some("off") => Self::Off,
        Some("unsupported") => Self::Unsupported,
        _ => Self::Failed(response.message.unwrap_or_default()),
      },
      other => Self::Failed(other.to_string()),
    }
  }
}

/// What the Kotlin half reports about a session, on the plugin's channel.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Report {
  /// The central wrote a chunk to `rx`.
  Chunk {
    session: u64,
    /// In base64.
    chunk: String,
    /// The link's ATT MTU.
    mtu: u16,
    /// The central's address, for the diagnostic log.
    from: String,
  },
  /// The central disconnected.
  Left { session: u64 },
}

/// What a session receives.
#[derive(Debug)]
enum Event {
  Chunk {
    chunk: Vec<u8>,
    mtu: u16,
    from: String,
  },
  Left,
}

/// Where the current session's reports go: one session at a time, so a
/// late report from an earlier one goes nowhere.
#[derive(Debug, Default)]
struct Route(Mutex<Option<(u64, mpsc::UnboundedSender<Event>)>>);

impl Route {
  fn lock(&self) -> std::sync::MutexGuard<'_, Option<(u64, mpsc::UnboundedSender<Event>)>> {
    // A poisoned lock still holds a valid route.
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }

  fn deliver(&self, report: Report) {
    let (session, event) = match report {
      Report::Chunk {
        session,
        chunk,
        mtu,
        from,
      } => match BASE64_STANDARD.decode(chunk) {
        Ok(chunk) => (session, Event::Chunk { chunk, mtu, from }),
        Err(error) => {
          log::warn!(error:% = error; "garbled chunk");
          (session, Event::Left)
        }
      },
      Report::Left { session } => (session, Event::Left),
    };
    if let Some((current, events)) = self.lock().as_ref()
      && *current == session
    {
      // The session is gone if nobody receives; the event with it.
      let _ = events.send(event);
    }
  }

  fn open(&self, session: u64) -> mpsc::UnboundedReceiver<Event> {
    let (events, received) = mpsc::unbounded_channel();
    *self.lock() = Some((session, events));
    received
  }

  fn close(&self, session: u64) {
    let mut route = self.lock();
    if route
      .as_ref()
      .is_some_and(|(current, _)| *current == session)
    {
      *route = None;
    }
  }
}

struct Shared<R: Runtime> {
  handle: PluginHandle<R>,
  route: Arc<Route>,
  /// The one channel the Kotlin half reports on, for the plugin's life.
  reports: Channel,
  next_session: AtomicU64,
}

#[derive(Serialize)]
struct StartArgs {
  session: u64,
  /// In base64; none to advertise for sync.
  token: Option<String>,
  events: Channel,
}

#[derive(Serialize)]
struct NotifyArgs {
  session: u64,
  /// In base64.
  chunk: String,
}

#[derive(Serialize)]
struct StopArgs {
  session: u64,
}

#[derive(Deserialize)]
struct Granted {
  granted: bool,
}

/// Runs a plugin call, which blocks until the Kotlin half answers, off the
/// async runtime's threads.
async fn call<T: Send + 'static>(
  call: impl FnOnce() -> Result<T, PluginInvokeError> + Send + 'static,
) -> Result<T, Error> {
  tauri::async_runtime::spawn_blocking(call)
    .await
    .map_err(|error| Error::Failed(error.to_string()))?
    .map_err(Error::from)
}

/// The plugin, as the app manages it.
pub struct BlePeripheral<R: Runtime>(Arc<Shared<R>>);

impl<R: Runtime> Clone for BlePeripheral<R> {
  fn clone(&self) -> Self {
    Self(Arc::clone(&self.0))
  }
}

impl<R: Runtime> BlePeripheral<R> {
  /// Makes sure the phone may advertise and serve: from Android 12 on,
  /// asks for the Nearby devices permission if it is not granted yet.
  ///
  /// # Errors
  ///
  /// [`Error::PermissionDenied`] if the user refuses it, or
  /// [`Error::Failed`].
  pub async fn ensure_permission(&self) -> Result<(), Error> {
    let shared = Arc::clone(&self.0);
    let answer: Granted =
      call(move || shared.handle.run_mobile_plugin("ensurePermission", ())).await?;
    if answer.granted {
      Ok(())
    } else {
      Err(Error::PermissionDenied)
    }
  }

  /// Starts a session: the phone advertises as `advertising` (opening
  /// Arsu's service first, the first time) until the session is dropped.
  /// A session started earlier stops advertising.
  ///
  /// # Errors
  ///
  /// [`Error::Off`], [`Error::Unsupported`] where the phone cannot
  /// advertise, or [`Error::Failed`].
  pub async fn start(&self, advertising: Advertising) -> Result<Session<R>, Error> {
    let session = self.0.next_session.fetch_add(1, Ordering::Relaxed);
    let events = self.0.route.open(session);
    let args = StartArgs {
      session,
      token: match advertising {
        Advertising::Sync => None,
        Advertising::Pairing(token) => Some(BASE64_STANDARD.encode(token)),
      },
      events: self.0.reports.clone(),
    };
    let shared = Arc::clone(&self.0);
    let started = call(move || shared.handle.run_mobile_plugin::<()>("start", args)).await;
    if let Err(error) = started {
      self.0.route.close(session);
      return Err(error);
    }
    Ok(Session {
      shared: Arc::clone(&self.0),
      id: session,
      events,
      central: None,
    })
  }
}

/// A session, advertising until dropped.
pub struct Session<R: Runtime> {
  shared: Arc<Shared<R>>,
  id: u64,
  events: mpsc::UnboundedReceiver<Event>,
  /// The central served, once it has written.
  central: Option<String>,
}

impl<R: Runtime> Session<R> {
  /// The central's next chunk, with the link's ATT MTU; `None` once it
  /// has disconnected.
  pub async fn next_chunk(&mut self) -> Option<(Vec<u8>, u16)> {
    match self.events.recv().await? {
      Event::Chunk { chunk, mtu, from } => {
        if self.central.is_none() {
          // The diagnostic log masks the address as it writes it.
          log::info!(central:% = from; "central wrote first");
          self.central = Some(from);
        }
        Some((chunk, mtu))
      }
      Event::Left => {
        log::info!("central disconnected");
        None
      }
    }
  }

  /// Notifies a chunk to the central, once Android has sent the previous.
  ///
  /// # Errors
  ///
  /// [`Error::Failed`] if no central listens, or Android refused.
  pub async fn notify(&self, chunk: &[u8]) -> Result<(), Error> {
    let args = NotifyArgs {
      session: self.id,
      chunk: BASE64_STANDARD.encode(chunk),
    };
    let shared = Arc::clone(&self.shared);
    call(move || shared.handle.run_mobile_plugin::<()>("notify", args)).await
  }
}

impl<R: Runtime> Drop for Session<R> {
  fn drop(&mut self) {
    self.shared.route.close(self.id);
    let shared = Arc::clone(&self.shared);
    let session = self.id;
    // Stopping takes a plugin call, which blocks: off the caller's thread.
    // The Kotlin half ignores it once a later session has started.
    drop(tauri::async_runtime::spawn_blocking(move || {
      if let Err(error) = shared
        .handle
        .run_mobile_plugin::<()>("stop", StopArgs { session })
      {
        log::warn!(error:% = error; "advertising not stopped");
      }
    }));
  }
}

/// `app.ble_peripheral()` reaches the plugin.
pub trait BlePeripheralExt<R: Runtime> {
  fn ble_peripheral(&self) -> &BlePeripheral<R>;
}

impl<R: Runtime, T: Manager<R>> BlePeripheralExt<R> for T {
  fn ble_peripheral(&self) -> &BlePeripheral<R> {
    self.state::<BlePeripheral<R>>().inner()
  }
}

/// The plugin, to register with the app's builder.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("arsu-ble-peripheral")
    .setup(|app, api| {
      let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "BlePeripheralPlugin")?;
      let route = Arc::new(Route::default());
      let reports = Channel::new({
        let route = Arc::clone(&route);
        move |body: InvokeResponseBody| {
          route.deliver(body.deserialize()?);
          Ok(())
        }
      });
      app.manage(BlePeripheral(Arc::new(Shared {
        handle,
        route,
        reports,
        next_session: AtomicU64::new(0),
      })));
      Ok(())
    })
    .build()
}
