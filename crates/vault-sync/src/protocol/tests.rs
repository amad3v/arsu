use otp::Algorithm;
use uuid::Uuid;
use vault_core::{DeviceSet, Entry, Member, OtpConfig, PUBLIC_KEY_LEN, PublicKey, Tombstone};

use super::*;

fn entry() -> Entry {
  Entry {
    id: Uuid::from_u128(1),
    issuer: Some("Example".to_string()),
    account_label: "alice".to_string(),
    otp: OtpConfig::Totp {
      secret: b"12345678901234567890".to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      period: 30,
    },
    icon: None,
    tags: vec!["work".to_string()],
    notes: None,
    created_at: 1,
    updated_at: 2,
  }
}

fn replica() -> Replica {
  Replica {
    vault_id: Uuid::from_u128(9),
    entries: vec![entry()],
    tombstones: vec![Tombstone::new(Uuid::from_u128(2), 3)],
  }
}

fn devices() -> DeviceSet {
  DeviceSet {
    members: vec![Member {
      public_key: PublicKey([4; PUBLIC_KEY_LEN]),
      name: "Phone".to_string(),
      added_at: 5,
    }],
    removed: Vec::new(),
  }
}

fn snapshot() -> Snapshot {
  Snapshot::new("Computer".to_string(), replica(), devices())
}

#[test]
fn a_new_snapshot_speaks_this_protocol_version() {
  let snapshot = snapshot();

  assert_eq!(snapshot.protocol, PROTOCOL_VERSION);
  assert_eq!(snapshot.sender_name, "Computer");
  assert_eq!(snapshot.replica(), replica());
  assert_eq!(snapshot.devices, devices());
}

#[test]
fn a_snapshot_comes_apart_into_name_replica_and_devices() {
  assert_eq!(
    snapshot().into_parts(),
    ("Computer".to_string(), replica(), devices())
  );
}

#[test]
fn messages_round_trip() {
  for message in [Message::Snapshot(Box::new(snapshot())), Message::Done] {
    let bytes = message.encode().unwrap();
    assert_eq!(Message::decode(&bytes).unwrap(), message);
  }
}

#[test]
fn a_snapshot_of_another_version_is_refused() {
  let mut future = snapshot();
  future.protocol = PROTOCOL_VERSION + 1;
  let bytes = Message::Snapshot(Box::new(future)).encode().unwrap();

  assert!(matches!(
    Message::decode(&bytes),
    Err(ProtocolError::UnsupportedProtocol(2))
  ));
}

#[test]
fn garbage_is_not_a_message() {
  assert!(matches!(
    Message::decode(b"\xa1\x63abc\x01"),
    Err(ProtocolError::Cbor(_))
  ));
  assert!(matches!(Message::decode(b""), Err(ProtocolError::Cbor(_))));
}

#[test]
fn too_many_tombstones_are_refused() {
  let mut huge = snapshot();
  huge.tombstones = (0..=MAX_TOMBSTONES as u128)
    .map(|n| Tombstone::new(Uuid::from_u128(n), 0))
    .collect();
  let bytes = Message::Snapshot(Box::new(huge)).encode().unwrap();

  assert!(matches!(
    Message::decode(&bytes),
    Err(ProtocolError::TooManyTombstones)
  ));
}

#[test]
fn unknown_fields_from_a_newer_minor_version_are_ignored() {
  #[derive(serde::Serialize)]
  struct Newer<'a> {
    #[serde(flatten)]
    snapshot: &'a Snapshot,
    added_later: &'a str,
  }
  #[derive(serde::Serialize)]
  enum NewerMessage<'a> {
    Snapshot(Newer<'a>),
  }
  let snapshot = snapshot();
  let bytes = cbor::to_zeroizing_vec(&NewerMessage::Snapshot(Newer {
    snapshot: &snapshot,
    added_later: "whatever",
  }))
  .unwrap();

  assert_eq!(
    Message::decode(&bytes).unwrap(),
    Message::Snapshot(Box::new(snapshot))
  );
}
