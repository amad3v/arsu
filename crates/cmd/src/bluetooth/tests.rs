use std::{collections::VecDeque, sync::Mutex};

use super::*;
use crate::{
  dto::SyncSummary,
  error::AppErrorKind,
  state::{
    LiveRole,
    fixture::{Fixture, secret},
    wire::{Wire, pair, side, wires},
  },
};

fn summary() -> SyncSummary {
  SyncSummary {
    peer_name: "Phone".to_string(),
    added: 1,
    updated: 0,
    deleted: 0,
    paired: false,
  }
}

#[test]
fn the_loop_goes_on_after_a_sync_or_a_failure_of_one_device() {
  assert!(serve_next(&Ok(summary())));
  assert!(serve_next(&Err(AppError::Disconnected)));
  assert!(serve_next(&Err(AppError::UntrustedDevice)));
}

#[test]
fn the_loop_stops_when_every_next_session_would_fail() {
  assert!(!serve_next(&Err(AppError::Locked)));
}

#[test]
fn reports_carry_their_kind_for_the_interface() {
  let finished = serde_json::to_value(SessionReport::of(Ok(summary()))).unwrap();
  assert_eq!(finished["kind"], "finished");
  assert_eq!(finished["summary"]["peerName"], "Phone");

  let failed = serde_json::to_value(SessionReport::of(Err(AppError::Disconnected))).unwrap();
  assert_eq!(failed["kind"], "failed");
  assert_eq!(failed["error"]["kind"], "BluetoothDisconnected");
  assert!(
    failed["error"]["message"]
      .as_str()
      .unwrap()
      .contains("disconnected")
  );
}

// ---- waiting for chunks ------------------------------------------------------

/// A peer that never sends anything.
fn silence() -> std::future::Pending<Option<Vec<u8>>> {
  std::future::pending()
}

#[tokio::test(start_paused = true)]
async fn waiting_for_a_device_has_no_time_limit() {
  let mut idle = Idle::waiting();

  // Far longer than the idle limit, with no device coming.
  let waited = tokio::time::timeout(IDLE_TIMEOUT * 100, idle.next(silence())).await;

  assert!(waited.is_err(), "still waiting");
}

#[tokio::test(start_paused = true)]
async fn once_a_session_has_started_a_silent_peer_counts_as_gone() {
  let mut idle = Idle::waiting();
  assert_eq!(
    idle.next(std::future::ready(Some(vec![1]))).await,
    Some(vec![1])
  );

  assert_eq!(idle.next(silence()).await, None);
}

#[tokio::test(start_paused = true)]
async fn a_central_has_connected_so_its_session_has_started() {
  assert_eq!(Idle::started().next(silence()).await, None);
}

// ---- the central -------------------------------------------------------------

/// A central that finds `found` and connects through one in-memory wire.
struct FakeCentral {
  found: Vec<FoundDevice>,
  wire: Mutex<Option<Wire>>,
  scans: Mutex<usize>,
  connected: Mutex<Vec<String>>,
}

impl FakeCentral {
  fn new(found: Vec<FoundDevice>, wire: Option<Wire>) -> Self {
    Self {
      found,
      wire: Mutex::new(wire),
      scans: Mutex::new(0),
      connected: Mutex::new(Vec::new()),
    }
  }
}

impl Central for FakeCentral {
  type Peer = Wire;

  fn scan(&self, _duration: Duration) -> impl Future<Output = Result<Vec<FoundDevice>, AppError>> {
    *self.scans.lock().unwrap() += 1;
    std::future::ready(Ok(self.found.clone()))
  }

  fn connect(&self, address: &str) -> impl Future<Output = Result<(Wire, usize), AppError>> {
    self.connected.lock().unwrap().push(address.to_owned());
    let wire = self.wire.lock().unwrap().take().expect("one connection");
    std::future::ready(Ok((wire, 185)))
  }

  async fn disconnect(&self, peer: Wire) {
    drop(peer);
  }
}

fn device(address: &str, token: Option<[u8; TOKEN_LEN]>) -> FoundDevice {
  FoundDevice {
    address: address.to_owned(),
    token,
  }
}

#[tokio::test]
async fn nearby_devices_are_listed_with_whether_they_wait_to_pair() {
  let central = FakeCentral::new(
    vec![device("A", None), device("B", Some([1; TOKEN_LEN]))],
    None,
  );

  let found = find_devices(&central).await.unwrap();

  assert_eq!(
    found,
    vec![
      NearbyDevice {
        address: "A".to_owned(),
        pairing: false
      },
      NearbyDevice {
        address: "B".to_owned(),
        pairing: true
      },
    ]
  );
}

#[tokio::test]
async fn joining_connects_to_the_device_that_shows_the_invitation() {
  let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
  computer.add_account("alice");
  let (invitation, _) = computer.state.create_invitation().unwrap();
  let token = advertisement_token(&invitation.secret);
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(
    vec![
      device("other", Some([0; TOKEN_LEN])),
      device("computer", Some(token)),
    ],
    Some(central_end),
  );
  let qr = invitation.qr_text();

  let (joined, invited) = tokio::join!(
    join(&phone.state, &central, &qr),
    side(&computer, LiveRole::Invite(invitation), peripheral_end, 0),
  );

  assert!(joined.unwrap().paired && invited.unwrap().paired);
  assert_eq!(*central.connected.lock().unwrap(), ["computer"]);
  assert_eq!(phone.labels(), ["alice"]);
  assert_eq!(phone.other_devices(), 1);
}

#[tokio::test]
async fn a_typed_code_joins_too() {
  let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
  let (invitation, _) = computer.state.create_invitation().unwrap();
  let token = advertisement_token(&invitation.secret);
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(vec![device("computer", Some(token))], Some(central_end));
  let code = invitation.code();

  let (joined, _) = tokio::join!(
    join(&phone.state, &central, &code),
    side(&computer, LiveRole::Invite(invitation), peripheral_end, 0),
  );

  assert!(joined.unwrap().paired);
}

#[tokio::test]
async fn joining_without_the_inviting_device_nearby_connects_to_nothing() {
  let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
  let (invitation, _) = computer.state.create_invitation().unwrap();
  let central = FakeCentral::new(vec![device("other", Some([0; TOKEN_LEN]))], None);

  let result = join(&phone.state, &central, &invitation.qr_text()).await;

  assert_eq!(
    result.unwrap_err().kind(),
    AppErrorKind::InvitingDeviceNotFound
  );
  assert!(central.connected.lock().unwrap().is_empty());
}

#[tokio::test]
async fn text_that_is_not_an_invitation_is_refused_before_any_scan() {
  let phone = Fixture::unlocked();
  let central = FakeCentral::new(Vec::new(), None);

  let result = join(&phone.state, &central, secret("hello").expose()).await;

  assert_eq!(result.unwrap_err().kind(), AppErrorKind::NotAnInvitation);
  assert_eq!(*central.scans.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_paired_device_syncs_as_the_central() {
  let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
  pair(&computer, &phone).await;
  phone.add_account("carol");
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(vec![device("computer", None)], Some(central_end));

  let (synced, served) = tokio::join!(
    sync_with(&phone.state, &central, "computer"),
    side(&computer, LiveRole::SyncResponder, peripheral_end, 0),
  );

  assert!(!synced.unwrap().paired);
  assert_eq!(served.unwrap().added, 1);
  assert_eq!(computer.labels(), ["carol"]);
}

#[tokio::test]
async fn a_device_paired_with_none_does_not_try_to_sync() {
  let phone = Fixture::unlocked();
  // No wire: connecting would fail the test.
  let central = FakeCentral::new(vec![device("computer", None)], None);

  let result = sync_with(&phone.state, &central, "computer").await;

  assert_eq!(result.unwrap_err().kind(), AppErrorKind::NothingPaired);
  assert!(central.connected.lock().unwrap().is_empty());
}

// ---- the peripheral ----------------------------------------------------------

/// A peripheral whose sessions are in-memory wires, one per session, and
/// which cannot advertise once they run out (as a phone that never could).
struct FakePeripheral {
  wires: VecDeque<Wire>,
  advertised: Vec<Advertising>,
}

impl FakePeripheral {
  fn new(wires: impl IntoIterator<Item = Wire>) -> Self {
    Self {
      wires: wires.into_iter().collect(),
      advertised: Vec::new(),
    }
  }
}

impl Peripheral for FakePeripheral {
  type Session<'p> = Wire;

  fn start_session(
    &mut self,
    advertising: Advertising,
  ) -> impl Future<Output = Result<Wire, AppError>> {
    self.advertised.push(advertising);
    std::future::ready(self.wires.pop_front().ok_or(AppError::CannotWait))
  }
}

/// The reports a serving loop sent, in order.
#[derive(Default)]
struct Reports(Mutex<Vec<SessionReport>>);

impl Reports {
  fn push(&self, report: SessionReport) {
    self.0.lock().unwrap().push(report);
  }

  fn kinds(&self) -> Vec<Result<bool, AppErrorKind>> {
    self
      .0
      .lock()
      .unwrap()
      .iter()
      .map(|report| match report {
        SessionReport::Finished { summary } => Ok(summary.paired),
        SessionReport::Failed { error } => Err(error.kind),
      })
      .collect()
  }
}

#[tokio::test]
async fn a_waiting_device_serves_each_paired_device_in_turn() {
  let (waiting, phone, tablet) = (
    Fixture::unlocked(),
    Fixture::unlocked(),
    Fixture::unlocked(),
  );
  pair(&waiting, &phone).await;
  pair(&waiting, &tablet).await;
  phone.add_account("phone");
  tablet.add_account("tablet");
  let ((phone_end, first), (tablet_end, second)) = (wires(185), wires(185));
  let mut peripheral = FakePeripheral::new([first, second]);
  let reports = Reports::default();

  let ((), from_phone, from_tablet) = tokio::join!(
    serve_sync(&waiting.state, &mut peripheral, |report| reports
      .push(report)),
    side(&phone, LiveRole::SyncInitiator, phone_end, 185),
    side(&tablet, LiveRole::SyncInitiator, tablet_end, 185),
  );

  assert!(from_phone.is_ok() && from_tablet.is_ok());
  assert_eq!(waiting.labels(), ["phone", "tablet"]);
  // The third session could not start, which ends the loop: said once.
  assert_eq!(
    reports.kinds(),
    [Ok(false), Ok(false), Err(AppErrorKind::BluetoothCannotWait)]
  );
  assert_eq!(peripheral.advertised, [Advertising::Sync; 3]);
}

#[tokio::test]
async fn a_waiting_device_advertises_its_invitation_and_pairs() {
  let (waiting, joining) = (Fixture::unlocked(), Fixture::unlocked());
  waiting.add_account("alice");
  let (invitation, _) = waiting.state.create_invitation().unwrap();
  let token = advertisement_token(&invitation.secret);
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(vec![device("waiting", Some(token))], Some(central_end));
  let mut peripheral = FakePeripheral::new([peripheral_end]);
  let reports = Reports::default();
  let code = invitation.code();

  let ((), joined) = tokio::join!(
    serve_pairing(&waiting.state, &mut peripheral, invitation, |report| {
      reports.push(report);
    }),
    join(&joining.state, &central, &code),
  );

  assert!(joined.unwrap().paired);
  assert_eq!(reports.kinds(), [Ok(true)]);
  assert_eq!(peripheral.advertised, [Advertising::Pairing(token)]);
  assert_eq!(joining.labels(), ["alice"]);
}

/// Phones only, then a computer: it pairs with one phone, and the others
/// learn of it the way every pairing spreads, through a sync with a
/// device that knows it.
#[tokio::test]
async fn a_computer_joins_phones_through_any_one_of_them() {
  let (phone_a, phone_b, computer) = (
    Fixture::unlocked(),
    Fixture::unlocked(),
    Fixture::unlocked(),
  );
  pair(&phone_a, &phone_b).await;
  phone_b.add_account("bob");
  // Phone A waits with an invitation; the computer, which has no camera,
  // types its code.
  let (invitation, _) = phone_a.state.create_invitation().unwrap();
  let token = advertisement_token(&invitation.secret);
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(vec![device("phone A", Some(token))], Some(central_end));
  let mut peripheral = FakePeripheral::new([peripheral_end]);
  let code = invitation.code();
  let ((), joined) = tokio::join!(
    serve_pairing(&phone_a.state, &mut peripheral, invitation, |_| {}),
    join(&computer.state, &central, &code),
  );
  assert!(joined.unwrap().paired);
  // The computer learnt of phone B from phone A, but phone B has not
  // heard of the computer yet, and refuses it.
  assert_eq!(computer.other_devices(), 2);
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(vec![device("phone B", None)], Some(central_end));
  let (refused, _) = tokio::join!(
    sync_with(&computer.state, &central, "phone B"),
    side(&phone_b, LiveRole::SyncResponder, peripheral_end, 0),
  );
  assert!(refused.is_err());

  // Phone B syncs with phone A, and so learns of the computer...
  let (central_end, peripheral_end) = wires(185);
  let (b_with_a, _) = tokio::join!(
    side(&phone_b, LiveRole::SyncInitiator, central_end, 185),
    side(&phone_a, LiveRole::SyncResponder, peripheral_end, 0),
  );
  b_with_a.unwrap();
  assert_eq!(phone_b.other_devices(), 2);

  // ...and now syncs with it, either one waiting.
  let (central_end, peripheral_end) = wires(185);
  let central = FakeCentral::new(vec![device("phone B", None)], Some(central_end));
  let (synced, _) = tokio::join!(
    sync_with(&computer.state, &central, "phone B"),
    side(&phone_b, LiveRole::SyncResponder, peripheral_end, 0),
  );
  synced.unwrap();
  assert_eq!(computer.labels(), ["bob"]);
}
