use std::{
  collections::{BTreeMap, BTreeSet},
  fmt,
  pin::Pin,
  sync::{Arc, Mutex, PoisonError},
};

use bluer::{
  Adapter, Address, DeviceEvent, DeviceProperty,
  adv::{Advertisement, AdvertisementHandle},
  gatt::local::{
    Application, ApplicationHandle, Characteristic, CharacteristicNotifier, CharacteristicNotify,
    CharacteristicNotifyMethod, CharacteristicWrite, CharacteristicWriteMethod, ReqError,
    ReqResult, Service,
  },
};
use futures_util::{Stream, StreamExt};
use tokio::sync::mpsc;
use vault_sync::gatt::{Advertising, RX_UUID, SERVICE_UUID, TX_UUID};

/// Chunks waiting for the app, at most: a full ATT window and then some.
const QUEUE_LEN: usize = 256;

/// What the central did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
  /// A chunk written to `rx` by the central at `from`, with the link's
  /// ATT MTU.
  Chunk {
    chunk: Vec<u8>,
    mtu: u16,
    from: Address,
  },
}

#[derive(Debug, thiserror::Error)]
pub enum BleError {
  #[error("Bluetooth is not available: {0}")]
  Bluetooth(#[from] bluer::Error),
  #[error("the other device has not subscribed to Arsu's notifications")]
  NotSubscribed,
  #[error("this device does not serve Arsu's sync service")]
  NotArsu,
}

/// Serves the first central that writes in a session, and refuses every
/// other until the next session; refuses the last session's central, if
/// it stayed connected, until it leaves.
///
/// A session that fails ends while its central is still writing the rest
/// of a message: let through to the next session, that rest would start
/// it out of order and end it at once, and the next, chunk after chunk.
/// Refused, the central learns at its next write that the session is
/// over. It is not disconnected: a session that succeeds ends with the
/// last message possibly still on its way to it.
#[derive(Debug, Default)]
pub struct CentralGate(Mutex<Gate>);

#[derive(Debug, Default)]
struct Gate {
  served: Option<Address>,
  dismissed: Option<Address>,
}

impl CentralGate {
  /// Whether a write from `address` may go through.
  pub fn admit(&self, address: Address) -> bool {
    let mut gate = self.gate();
    gate.dismissed != Some(address) && *gate.served.get_or_insert(address) == address
  }

  /// Opens the gate to whichever central writes first next; the one
  /// served until now, still connected, stays out until it leaves
  /// ([`Self::left`]).
  pub fn next_session(&self, served_still_connected: bool) {
    let mut gate = self.gate();
    let served = gate.served.take();
    if served_still_connected {
      gate.dismissed = served;
    }
  }

  /// Notes that the central at `address` has left: it may write again
  /// once it is back.
  pub fn left(&self, address: Address) {
    let mut gate = self.gate();
    if gate.dismissed == Some(address) {
      gate.dismissed = None;
    }
  }

  fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
    // A poisoned lock still holds valid addresses.
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// Whether `event`, about the central being served, says it has gone:
/// `BlueZ` no longer counts it as connected.
#[must_use]
pub fn has_left(event: &DeviceEvent) -> bool {
  matches!(
    event,
    DeviceEvent::PropertyChanged(DeviceProperty::Connected(false))
  )
}

/// The advertisement for `advertising`.
#[must_use]
pub fn advertisement(advertising: Advertising) -> Advertisement {
  let (service_uuids, service_data) = match advertising {
    Advertising::Sync => (BTreeSet::from([SERVICE_UUID]), BTreeMap::new()),
    Advertising::Pairing(token) => (
      BTreeSet::new(),
      BTreeMap::from([(SERVICE_UUID, token.to_vec())]),
    ),
  };
  Advertisement {
    service_uuids,
    service_data,
    discoverable: Some(true),
    ..Advertisement::default()
  }
}

/// A write from the central, as the GATT application receives it: passed
/// on as an [`Event::Chunk`] if `address` is the central being served.
///
/// # Errors
///
/// [`ReqError::NotPermitted`] for any other device; [`ReqError::Failed`]
/// if the app stopped listening.
pub fn accept_write(
  gate: &CentralGate,
  events: &mpsc::Sender<Event>,
  address: Address,
  chunk: Vec<u8>,
  mtu: u16,
) -> ReqResult<()> {
  if !gate.admit(address) {
    return Err(ReqError::NotPermitted);
  }
  // Answering the write only once the chunk is queued is the flow
  // control: the central sends the next one after the answer.
  events
    .try_send(Event::Chunk {
      chunk,
      mtu,
      from: address,
    })
    .map_err(|_| ReqError::Failed)
}

/// The GATT application: Arsu's service, with `rx` (written by the
/// central, with response) and `tx` (notified to it).
#[must_use]
pub fn application(
  gate: Arc<CentralGate>,
  events: mpsc::Sender<Event>,
  notifier: Arc<tokio::sync::Mutex<Option<CharacteristicNotifier>>>,
) -> Application {
  let rx = Characteristic {
    uuid: RX_UUID,
    write: Some(CharacteristicWrite {
      write: true,
      method: CharacteristicWriteMethod::Fun(Box::new(move |chunk, request| {
        let result = accept_write(&gate, &events, request.device_address, chunk, request.mtu);
        Box::pin(async move { result })
      })),
      ..CharacteristicWrite::default()
    }),
    ..Characteristic::default()
  };
  let tx = Characteristic {
    uuid: TX_UUID,
    notify: Some(CharacteristicNotify {
      notify: true,
      method: CharacteristicNotifyMethod::Fun(Box::new(move |subscribed| {
        let notifier = Arc::clone(&notifier);
        Box::pin(async move {
          *notifier.lock().await = Some(subscribed);
        })
      })),
      ..CharacteristicNotify::default()
    }),
    ..Characteristic::default()
  };
  Application {
    services: vec![Service {
      uuid: SERVICE_UUID,
      primary: true,
      characteristics: vec![rx, tx],
      ..Service::default()
    }],
    ..Application::default()
  }
}

/// News of the central being served, from `BlueZ`.
type DeviceEvents = Pin<Box<dyn Stream<Item = DeviceEvent> + Send>>;

/// Arsu's GATT service on this computer: registered once and kept for as
/// long as the app runs, so that its attribute handles never change. A
/// phone remembers a device's handles between connections, and writing
/// to one a re-registration moved fails (`GATT_INVALID_HANDLE`).
/// Advertising is separate, one [`Advertised`] per session, and is what
/// makes the service findable.
pub struct GattServer {
  adapter: Adapter,
  gate: Arc<CentralGate>,
  events: mpsc::Receiver<Event>,
  notifier: Arc<tokio::sync::Mutex<Option<CharacteristicNotifier>>>,
  /// The session's central, once it has written, and its news.
  central: Option<(Address, DeviceEvents)>,
  /// The last session's central, while it stays connected, and its news.
  dismissed: Option<(Address, DeviceEvents)>,
  _application: ApplicationHandle,
}

impl fmt::Debug for GattServer {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("GattServer")
      .field("adapter", &self.adapter.name())
      .finish_non_exhaustive()
  }
}

/// A session's advertisement: advertising stops when it is dropped.
#[derive(Debug)]
pub struct Advertised {
  _handle: AdvertisementHandle,
}

impl GattServer {
  /// Powers the default adapter on and registers Arsu's service.
  ///
  /// # Errors
  ///
  /// [`BleError::Bluetooth`] if there is no adapter, `bluetoothd` is not
  /// running, or it refuses the application.
  pub async fn register() -> Result<Self, BleError> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    let (events_sender, events) = mpsc::channel(QUEUE_LEN);
    let gate = Arc::new(CentralGate::default());
    let notifier = Arc::new(tokio::sync::Mutex::new(None));
    let application = adapter
      .serve_gatt_application(application(
        Arc::clone(&gate),
        events_sender,
        Arc::clone(&notifier),
      ))
      .await?;
    Ok(Self {
      adapter,
      gate,
      events,
      notifier,
      central: None,
      dismissed: None,
      _application: application,
    })
  }

  /// Starts a session: forgets anything the last one left behind and
  /// keeps its central out while it stays connected ([`CentralGate`]),
  /// then advertises.
  ///
  /// # Errors
  ///
  /// [`BleError::Bluetooth`] if `bluetoothd` refuses the advertisement.
  pub async fn start_session(&mut self, advertising: Advertising) -> Result<Advertised, BleError> {
    let still_connected = match &self.central {
      Some((address, _)) => is_connected(&self.adapter, *address).await,
      None => false,
    };
    self.gate.next_session(still_connected);
    if still_connected {
      self.dismissed = self.central.take();
    } else {
      self.central = None;
    }
    *self.notifier.lock().await = None;
    while self.events.try_recv().is_ok() {}
    let handle = self.adapter.advertise(advertisement(advertising)).await?;
    // The address a central connects to, to match against its own log
    // (not the adapter's name, which is usually the computer's); the
    // diagnostic log masks it as it writes it.
    if let Ok(address) = self.adapter.address().await {
      log::info!(adapter:% = address; "advertising");
    }
    Ok(Advertised { _handle: handle })
  }

  /// The session's next chunk, with the link's ATT MTU; `None` once its
  /// central has disconnected.
  pub async fn next_chunk(&mut self) -> Option<(Vec<u8>, u16)> {
    loop {
      tokio::select! {
        event = self.events.recv() => {
          let Event::Chunk { chunk, mtu, from } = event?;
          if self.central.is_none() {
            log::info!(central:% = from; "central wrote first");
            self.central = Some((from, watch(&self.adapter, from).await?));
          }
          return Some((chunk, mtu));
        }
        () = left(self.central.as_mut().map(|(_, events)| events)) => {
          log::info!("central disconnected");
          return None;
        }
        () = left(self.dismissed.as_mut().map(|(_, events)| events)) => {
          if let Some((address, _)) = self.dismissed.take() {
            self.gate.left(address);
          }
        }
      }
    }
  }

  /// Notifies a chunk to the central.
  ///
  /// # Errors
  ///
  /// [`BleError::NotSubscribed`] before the central subscribed to `tx`;
  /// [`BleError::Bluetooth`] if the notification fails.
  pub async fn send(&mut self, chunk: Vec<u8>) -> Result<(), BleError> {
    let mut notifier = self.notifier.lock().await;
    let notifier = notifier.as_mut().ok_or(BleError::NotSubscribed)?;
    Ok(notifier.notify(chunk).await?)
  }
}

/// `BlueZ`'s news of the central at `address`; `None` if it is already
/// gone.
async fn watch(adapter: &Adapter, address: Address) -> Option<DeviceEvents> {
  let device = adapter.device(address).ok()?;
  let events = device.events().await.ok()?;
  // Gone before the watch began: nothing would ever say so.
  if !device.is_connected().await.unwrap_or(false) {
    log::info!(central:% = address; "central left before its first write was read");
    return None;
  }
  Some(Box::pin(events))
}

/// Whether `BlueZ` counts the device at `address` as connected.
async fn is_connected(adapter: &Adapter, address: Address) -> bool {
  match adapter.device(address) {
    Ok(device) => device.is_connected().await.unwrap_or(false),
    Err(_) => false,
  }
}

/// Resolves once the central of `events` has left; never without one.
async fn left(events: Option<&mut DeviceEvents>) {
  let Some(events) = events else {
    return std::future::pending().await;
  };
  while let Some(event) = events.next().await {
    if has_left(&event) {
      return;
    }
  }
  // `BlueZ` dropped the device: it is gone too.
}

#[cfg(test)]
mod tests {
  use vault_sync::invitation::TOKEN_LEN;

  use super::*;

  const PHONE: Address = Address([1, 2, 3, 4, 5, 6]);
  const OTHER: Address = Address([9, 9, 9, 9, 9, 9]);

  #[test]
  fn sync_mode_advertises_the_service_and_nothing_about_the_device() {
    let advertisement = advertisement(Advertising::Sync);

    assert_eq!(advertisement.service_uuids, BTreeSet::from([SERVICE_UUID]));
    assert!(advertisement.service_data.is_empty());
    assert!(advertisement.manufacturer_data.is_empty());
    assert_eq!(advertisement.local_name, None);
    assert_eq!(advertisement.discoverable, Some(true));
  }

  #[test]
  fn pairing_mode_carries_the_token_in_place_of_the_service_list() {
    let advertisement = advertisement(Advertising::Pairing([7; TOKEN_LEN]));

    assert!(advertisement.service_uuids.is_empty());
    assert_eq!(
      advertisement.service_data,
      BTreeMap::from([(SERVICE_UUID, vec![7; TOKEN_LEN])])
    );
    assert_eq!(advertisement.local_name, None);
    assert_eq!(advertisement.discoverable, Some(true));
  }

  /// The bytes `advertisement` takes in an advertising packet (Bluetooth
  /// Core spec, vol 3 part C §11, and the Supplement's AD types): each AD
  /// structure is a length byte, a type byte and its data. Arsu's UUIDs
  /// are 128-bit; `BlueZ` adds the flags for a discoverable advertisement.
  fn packet_len(advertisement: &Advertisement) -> usize {
    const AD_HEADER: usize = 2;
    const UUID_128: usize = 16;
    let flags = if advertisement.discoverable == Some(true) {
      AD_HEADER + 1
    } else {
      0
    };
    let services = if advertisement.service_uuids.is_empty() {
      0
    } else {
      AD_HEADER + UUID_128 * advertisement.service_uuids.len()
    };
    let service_data: usize = advertisement
      .service_data
      .values()
      .map(|data| AD_HEADER + UUID_128 + data.len())
      .sum();
    let manufacturer_data: usize = advertisement
      .manufacturer_data
      .values()
      .map(|data| AD_HEADER + 2 + data.len())
      .sum();
    let name = advertisement
      .local_name
      .as_ref()
      .map_or(0, |name| AD_HEADER + name.len());
    flags + services + service_data + manufacturer_data + name
  }

  #[test]
  fn every_advertisement_fits_a_legacy_advertising_packet() {
    // Phones scan for legacy advertisements only: a longer one goes out
    // as an extended advertisement, which they never see.
    const LEGACY_ADVERTISING_DATA: usize = 31;

    for advertising in [Advertising::Sync, Advertising::Pairing([7; TOKEN_LEN])] {
      let len = packet_len(&advertisement(advertising));
      assert!(
        len <= LEGACY_ADVERTISING_DATA,
        "{advertising:?} takes {len} bytes"
      );
    }
  }

  #[test]
  fn a_new_session_serves_whichever_central_writes_first() {
    let gate = CentralGate::default();
    assert!(gate.admit(PHONE));

    gate.next_session(false);

    assert!(gate.admit(OTHER));
    assert!(!gate.admit(PHONE));
  }

  #[test]
  fn the_last_central_still_connected_is_refused_until_it_leaves() {
    let gate = CentralGate::default();
    assert!(gate.admit(PHONE));

    gate.next_session(true);

    assert!(!gate.admit(PHONE), "the rest of its last message");
    gate.left(PHONE);
    assert!(gate.admit(PHONE), "back for a new session");
  }

  #[test]
  fn the_last_central_still_connected_leaves_room_for_another() {
    let gate = CentralGate::default();
    assert!(gate.admit(PHONE));

    gate.next_session(true);

    assert!(gate.admit(OTHER));
    assert!(!gate.admit(PHONE));
  }

  #[test]
  fn the_central_has_left_once_it_is_no_longer_connected() {
    assert!(has_left(&DeviceEvent::PropertyChanged(
      DeviceProperty::Connected(false)
    )));
  }

  #[test]
  fn other_news_of_the_central_is_not_its_leaving() {
    assert!(!has_left(&DeviceEvent::PropertyChanged(
      DeviceProperty::Connected(true)
    )));
    assert!(!has_left(&DeviceEvent::PropertyChanged(
      DeviceProperty::Rssi(-60)
    )));
  }

  #[test]
  fn the_first_central_is_served_and_others_refused() {
    let gate = CentralGate::default();

    assert!(gate.admit(PHONE));
    assert!(gate.admit(PHONE));
    assert!(!gate.admit(OTHER));
  }

  #[tokio::test]
  async fn a_write_from_the_central_becomes_an_event() {
    let gate = CentralGate::default();
    let (events, mut received) = mpsc::channel(4);

    accept_write(&gate, &events, PHONE, vec![0, 1, 2], 185).unwrap();

    assert_eq!(
      received.recv().await,
      Some(Event::Chunk {
        chunk: vec![0, 1, 2],
        mtu: 185,
        from: PHONE,
      })
    );
  }

  #[test]
  fn a_write_from_another_device_is_refused() {
    let gate = CentralGate::default();
    let (events, _received) = mpsc::channel(4);
    accept_write(&gate, &events, PHONE, vec![0], 23).unwrap();

    assert!(matches!(
      accept_write(&gate, &events, OTHER, vec![0], 23),
      Err(ReqError::NotPermitted)
    ));
  }

  #[test]
  fn a_write_nobody_listens_to_fails() {
    let gate = CentralGate::default();
    let (events, received) = mpsc::channel(4);
    drop(received);

    assert!(matches!(
      accept_write(&gate, &events, PHONE, vec![0], 23),
      Err(ReqError::Failed)
    ));
  }

  #[test]
  fn the_application_serves_rx_and_tx_in_arsus_service() {
    let (events, _received) = mpsc::channel(4);
    let app = application(
      Arc::new(CentralGate::default()),
      events,
      Arc::new(tokio::sync::Mutex::new(None)),
    );

    assert_eq!(app.services.len(), 1);
    let service = &app.services[0];
    assert_eq!(service.uuid, SERVICE_UUID);
    assert!(service.primary);
    let rx = service
      .characteristics
      .iter()
      .find(|c| c.uuid == RX_UUID)
      .unwrap();
    let write = rx.write.as_ref().unwrap();
    assert!(write.write && !write.write_without_response);
    assert!(rx.notify.is_none() && rx.read.is_none());
    let tx = service
      .characteristics
      .iter()
      .find(|c| c.uuid == TX_UUID)
      .unwrap();
    assert!(tx.notify.as_ref().unwrap().notify);
    assert!(tx.write.is_none() && tx.read.is_none());
  }
}
