//! Sessions between two in-memory peers.

use otp::Algorithm;
use uuid::Uuid;
use vault_core::{DeviceIdentity, DeviceSet, Entry, Member, OtpConfig, PublicKey};

use super::*;
use crate::{Replica, noise::new_identity};

fn identity(name: &str) -> DeviceIdentity {
  new_identity(Uuid::now_v7(), name.to_string()).unwrap()
}

fn entry(n: u128, label: &str) -> Entry {
  Entry {
    id: Uuid::from_u128(n),
    issuer: None,
    account_label: label.to_string(),
    otp: OtpConfig::Totp {
      secret: label.as_bytes().to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      period: 30,
    },
    icon: None,
    tags: Vec::new(),
    notes: None,
    created_at: 1,
    updated_at: 1,
  }
}

fn trusting(keys: &[PublicKey]) -> DeviceSet {
  let mut set = DeviceSet::default();
  for key in keys {
    devices::add(
      &mut set,
      Member {
        public_key: *key,
        name: "Device".to_string(),
        added_at: 1,
      },
    )
    .unwrap();
  }
  set
}

fn snapshot(identity: &DeviceIdentity, entries: Vec<Entry>, devices: DeviceSet) -> Snapshot {
  Snapshot::new(
    identity.name.clone(),
    Replica {
      vault_id: Uuid::from_u128(1),
      entries,
      tombstones: Vec::new(),
    },
    devices,
  )
}

/// A side of a test session: the session, what it was told to send but
/// has not been delivered yet, and every other output it produced.
struct Peer {
  session: Session,
  outbox: Vec<u8>,
  received: Vec<(PublicKey, Snapshot)>,
  finished: bool,
  error: Option<SessionError>,
}

impl Peer {
  fn start(mode: Mode, identity: &DeviceIdentity, snapshot: Snapshot) -> Self {
    let (session, outputs) = Session::new(mode, identity, snapshot).unwrap();
    let mut peer = Self {
      session,
      outbox: Vec::new(),
      received: Vec::new(),
      finished: false,
      error: None,
    };
    peer.take(Ok(outputs));
    peer
  }

  fn take(&mut self, outputs: Result<Vec<Output>, SessionError>) {
    match outputs {
      Ok(outputs) => {
        for output in outputs {
          match output {
            Output::Send(bytes) => self.outbox.extend(bytes),
            Output::Received { peer, snapshot } => {
              self.received.push((peer, *snapshot));
              // What the caller does: merge, save, acknowledge.
              let more = self.session.acknowledge();
              self.take(more);
            }
            Output::Finished => self.finished = true,
          }
        }
      }
      Err(error) => self.error = Some(error),
    }
  }

  fn deliver(&mut self, bytes: &[u8], chunk: usize) {
    for piece in bytes.chunks(chunk) {
      if self.error.is_some() {
        return;
      }
      let outputs = self.session.receive(piece);
      self.take(outputs);
    }
  }
}

/// Runs the link until neither side has anything left to send, cutting
/// the byte stream into pieces of `chunk` bytes.
fn run(a: &mut Peer, b: &mut Peer, chunk: usize) {
  while !(a.outbox.is_empty() && b.outbox.is_empty()) {
    let to_b = std::mem::take(&mut a.outbox);
    b.deliver(&to_b, chunk);
    let to_a = std::mem::take(&mut b.outbox);
    a.deliver(&to_a, chunk);
  }
}

#[test]
fn paired_devices_exchange_snapshots_and_finish() {
  let (computer, phone) = (identity("Computer"), identity("Phone"));
  let both = trusting(&[computer.public_key, phone.public_key]);
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &phone,
    snapshot(&phone, vec![entry(1, "alice")], both.clone()),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &computer,
    snapshot(&computer, vec![entry(2, "bob")], both),
  );

  run(&mut central, &mut peripheral, usize::MAX);

  assert!(central.error.is_none() && peripheral.error.is_none());
  assert!(central.finished && peripheral.finished);
  let (from, snapshot) = &central.received[0];
  assert_eq!(*from, computer.public_key);
  assert_eq!(snapshot.entries, vec![entry(2, "bob")]);
  assert_eq!(snapshot.sender_name, "Computer");
  let (from, snapshot) = &peripheral.received[0];
  assert_eq!(*from, phone.public_key);
  assert_eq!(snapshot.entries, vec![entry(1, "alice")]);
  assert_eq!(central.session.peer(), Some(computer.public_key));
}

#[test]
fn a_session_survives_the_link_cutting_bytes_anywhere() {
  let (a, b) = (identity("A"), identity("B"));
  let both = trusting(&[a.public_key, b.public_key]);
  let many: Vec<Entry> = (0..2_000)
    .map(|n| entry(n, &format!("account {n}")))
    .collect();
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &a,
    snapshot(&a, many.clone(), both.clone()),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &b,
    snapshot(&b, vec![], both),
  );

  // 20 bytes at a time, like a BLE link with the smallest MTU.
  run(&mut central, &mut peripheral, 20);

  assert!(central.finished && peripheral.finished);
  assert_eq!(
    peripheral.received[0].1.entries, many,
    "spans many Noise messages"
  );
}

#[test]
fn an_untrusted_responder_never_learns_who_the_initiator_is() {
  let (phone, stranger) = (identity("Phone"), identity("Stranger"));
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &phone,
    snapshot(
      &phone,
      vec![entry(1, "secret")],
      trusting(&[phone.public_key]),
    ),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &stranger,
    snapshot(
      &stranger,
      vec![],
      trusting(&[stranger.public_key, phone.public_key]),
    ),
  );

  run(&mut central, &mut peripheral, usize::MAX);

  assert!(matches!(
    central.error,
    Some(SessionError::UntrustedPeer(key)) if key == stranger.public_key
  ));
  assert_eq!(peripheral.session.peer(), None, "message 3 was never sent");
  assert!(peripheral.received.is_empty());
}

#[test]
fn an_untrusted_initiator_gets_nothing() {
  let (computer, stranger) = (identity("Computer"), identity("Stranger"));
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &stranger,
    snapshot(
      &stranger,
      vec![],
      trusting(&[stranger.public_key, computer.public_key]),
    ),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &computer,
    snapshot(
      &computer,
      vec![entry(1, "secret")],
      trusting(&[computer.public_key]),
    ),
  );

  run(&mut central, &mut peripheral, usize::MAX);

  assert!(matches!(
    peripheral.error,
    Some(SessionError::UntrustedPeer(key)) if key == stranger.public_key
  ));
  assert!(
    central.received.is_empty(),
    "the computer's snapshot never left"
  );
}

#[test]
fn a_removed_device_is_refused() {
  let (computer, phone) = (identity("Computer"), identity("Phone"));
  let mut set = trusting(&[computer.public_key, phone.public_key]);
  devices::remove(&mut set, phone.public_key, 5);
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &phone,
    snapshot(
      &phone,
      vec![],
      trusting(&[computer.public_key, phone.public_key]),
    ),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &computer,
    snapshot(&computer, vec![], set),
  );

  run(&mut central, &mut peripheral, usize::MAX);

  assert!(matches!(
    peripheral.error,
    Some(SessionError::UntrustedPeer(_))
  ));
}

#[test]
fn a_device_talking_to_its_own_identity_stops() {
  let me = identity("Me");
  let mine = trusting(&[me.public_key]);
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &me,
    snapshot(&me, vec![], mine.clone()),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &me,
    snapshot(&me, vec![], mine),
  );

  run(&mut central, &mut peripheral, usize::MAX);

  assert!(matches!(central.error, Some(SessionError::OwnKey)));
}

#[test]
fn pairing_with_the_right_secret_introduces_both_devices() {
  let (computer, phone) = (identity("Computer"), identity("Phone"));
  let secret = PairingSecret::from_bytes([9; noise::SECRET_LEN]);
  let mut joiner = Peer::start(
    Mode::Join {
      secret: secret.clone(),
      inviter: Some(computer.public_key),
    },
    &phone,
    snapshot(&phone, vec![entry(1, "alice")], DeviceSet::default()),
  );
  let mut inviter = Peer::start(
    Mode::Invite { secret },
    &computer,
    snapshot(
      &computer,
      vec![entry(2, "bob")],
      trusting(&[computer.public_key]),
    ),
  );

  run(&mut joiner, &mut inviter, usize::MAX);

  assert!(joiner.finished && inviter.finished);
  assert_eq!(inviter.received[0].0, phone.public_key, "the device to add");
  assert_eq!(joiner.received[0].0, computer.public_key);
  assert_eq!(
    joiner.received[0].1.devices,
    trusting(&[computer.public_key])
  );
}

#[test]
fn pairing_without_the_inviter_key_works_too() {
  let (a, b) = (identity("A"), identity("B"));
  let secret = PairingSecret::from_bytes([3; noise::SECRET_LEN]);
  let mut joiner = Peer::start(
    Mode::Join {
      secret: secret.clone(),
      inviter: None,
    },
    &a,
    snapshot(&a, vec![], DeviceSet::default()),
  );
  let mut inviter = Peer::start(
    Mode::Invite { secret },
    &b,
    snapshot(&b, vec![], DeviceSet::default()),
  );

  run(&mut joiner, &mut inviter, usize::MAX);

  assert!(joiner.finished && inviter.finished);
}

#[test]
fn pairing_with_a_wrong_secret_fails_before_anything_is_exchanged() {
  let (computer, phone) = (identity("Computer"), identity("Phone"));
  let mut joiner = Peer::start(
    Mode::Join {
      secret: PairingSecret::from_bytes([1; noise::SECRET_LEN]),
      inviter: None,
    },
    &phone,
    snapshot(&phone, vec![entry(1, "alice")], DeviceSet::default()),
  );
  let mut inviter = Peer::start(
    Mode::Invite {
      secret: PairingSecret::from_bytes([2; noise::SECRET_LEN]),
    },
    &computer,
    snapshot(&computer, vec![entry(2, "bob")], DeviceSet::default()),
  );

  run(&mut joiner, &mut inviter, usize::MAX);

  assert!(matches!(inviter.error, Some(SessionError::Noise(_))));
  assert!(inviter.received.is_empty() && joiner.received.is_empty());
}

#[test]
fn pairing_refuses_a_device_other_than_the_one_in_the_invitation() {
  let (computer, impostor, phone) = (
    identity("Computer"),
    identity("Impostor"),
    identity("Phone"),
  );
  let secret = PairingSecret::from_bytes([4; noise::SECRET_LEN]);
  let mut joiner = Peer::start(
    Mode::Join {
      secret: secret.clone(),
      inviter: Some(computer.public_key),
    },
    &phone,
    snapshot(&phone, vec![], DeviceSet::default()),
  );
  let mut inviter = Peer::start(
    Mode::Invite { secret },
    &impostor,
    snapshot(&impostor, vec![], DeviceSet::default()),
  );

  run(&mut joiner, &mut inviter, usize::MAX);

  assert!(matches!(
    joiner.error,
    Some(SessionError::UnexpectedPeer(key)) if key == impostor.public_key
  ));
}

#[test]
fn a_pairing_session_cannot_talk_to_a_sync_session() {
  let (a, b) = (identity("A"), identity("B"));
  let both = trusting(&[a.public_key, b.public_key]);
  let mut joiner = Peer::start(
    Mode::Join {
      secret: PairingSecret::from_bytes([5; noise::SECRET_LEN]),
      inviter: None,
    },
    &a,
    snapshot(&a, vec![], both.clone()),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &b,
    snapshot(&b, vec![], both),
  );

  run(&mut joiner, &mut peripheral, usize::MAX);

  assert!(joiner.error.is_some() || peripheral.error.is_some());
  assert!(joiner.received.is_empty() && peripheral.received.is_empty());
}

#[test]
fn a_tampered_byte_ends_the_session() {
  let (a, b) = (identity("A"), identity("B"));
  let both = trusting(&[a.public_key, b.public_key]);
  let mut central = Peer::start(
    Mode::Sync { initiator: true },
    &a,
    snapshot(&a, vec![entry(1, "x")], both.clone()),
  );
  let mut peripheral = Peer::start(
    Mode::Sync { initiator: false },
    &b,
    snapshot(&b, vec![], both),
  );
  // Handshake: message 1, 2, then 3 and the snapshot.
  let first = std::mem::take(&mut central.outbox);
  peripheral.deliver(&first, usize::MAX);
  let second = std::mem::take(&mut peripheral.outbox);
  central.deliver(&second, usize::MAX);
  let mut third = std::mem::take(&mut central.outbox);
  let last = third.len() - 1;
  third[last] ^= 1;

  peripheral.deliver(&third, usize::MAX);

  assert!(matches!(peripheral.error, Some(SessionError::Noise(_))));
  assert!(peripheral.received.is_empty());
  assert!(matches!(
    peripheral.session.receive(&[0]),
    Err(SessionError::Closed)
  ));
}

#[test]
fn acknowledging_without_a_snapshot_is_an_error() {
  let a = identity("A");
  let (mut session, _) = Session::new(
    Mode::Sync { initiator: true },
    &a,
    snapshot(&a, vec![], DeviceSet::default()),
  )
  .unwrap();

  assert!(matches!(
    session.acknowledge(),
    Err(SessionError::UnexpectedMessage)
  ));
}

#[test]
fn refusing_a_device_names_it_by_its_fingerprint() {
  let key = PublicKey([0x12; 32]);

  let message = SessionError::UntrustedPeer(key).to_string();

  assert!(message.contains(&key.fingerprint()), "{message}");
}
