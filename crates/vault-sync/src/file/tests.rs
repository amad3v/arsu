use otp::Algorithm;
use uuid::Uuid;
use vault_core::{DeviceIdentity, DeviceSet, Entry, Member, OtpConfig, PublicKey};

use super::*;
use crate::{Replica, devices, noise::new_identity};

fn identity(name: &str) -> DeviceIdentity {
  new_identity(Uuid::now_v7(), name.to_string()).unwrap()
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

fn snapshot(sender: &DeviceIdentity, count: u128) -> Snapshot {
  let entries = (0..count)
    .map(|n| Entry {
      id: Uuid::from_u128(n),
      issuer: None,
      account_label: format!("account {n}"),
      otp: OtpConfig::Totp {
        secret: n.to_be_bytes().to_vec().into(),
        algorithm: Algorithm::Sha1,
        digits: 6,
        period: 30,
      },
      icon: None,
      tags: Vec::new(),
      notes: None,
      created_at: 1,
      updated_at: 1,
    })
    .collect();
  Snapshot::new(
    sender.name.clone(),
    Replica {
      vault_id: Uuid::from_u128(1),
      entries,
      tombstones: Vec::new(),
    },
    trusting(&[sender.public_key]),
  )
}

// ---- sync files -------------------------------------------------------------

#[test]
fn a_sync_file_is_read_by_its_recipient() {
  let (computer, phone) = (identity("Computer"), identity("Phone"));
  let sent = snapshot(&computer, 3);
  let file = write_sync_file(&computer, phone.public_key, sent.clone()).unwrap();

  let read = read_file(&file, &phone, &trusting(&[computer.public_key]), &[]).unwrap();

  assert!(file.starts_with(b"ARSY\x00\x01\x01"));
  assert_eq!(read.sender, computer.public_key);
  assert_eq!(read.snapshot, sent);
  assert!(read.invitation.is_none());
}

#[test]
fn a_large_vault_fits_in_a_sync_file() {
  let (a, b) = (identity("A"), identity("B"));
  let sent = snapshot(&a, 3_000);
  let file = write_sync_file(&a, b.public_key, sent.clone()).unwrap();

  assert_eq!(
    read_file(&file, &b, &trusting(&[a.public_key]), &[])
      .unwrap()
      .snapshot,
    sent
  );
}

#[test]
fn a_sync_file_cannot_be_read_by_another_device() {
  let (computer, phone, other) = (identity("Computer"), identity("Phone"), identity("Other"));
  let file = write_sync_file(&computer, phone.public_key, snapshot(&computer, 1)).unwrap();

  assert!(matches!(
    read_file(&file, &other, &trusting(&[computer.public_key]), &[]),
    Err(FileError::NotForThisDevice)
  ));
}

#[test]
fn a_sync_file_from_an_untrusted_device_is_refused() {
  let (stranger, phone) = (identity("Stranger"), identity("Phone"));
  let file = write_sync_file(&stranger, phone.public_key, snapshot(&stranger, 1)).unwrap();

  assert!(matches!(
    read_file(&file, &phone, &trusting(&[phone.public_key]), &[]),
    Err(FileError::UntrustedSender(key)) if key == stranger.public_key
  ));
}

#[test]
fn a_device_cannot_write_a_sync_file_for_itself() {
  let me = identity("Me");

  assert!(matches!(
    write_sync_file(&me, me.public_key, snapshot(&me, 1)),
    Err(FileError::OwnFile)
  ));
}

#[test]
fn reading_the_same_sync_file_twice_gives_the_same_snapshot() {
  // Replaying a file is harmless: merging is idempotent.
  let (a, b) = (identity("A"), identity("B"));
  let file = write_sync_file(&a, b.public_key, snapshot(&a, 2)).unwrap();
  let trusted = trusting(&[a.public_key]);

  let first = read_file(&file, &b, &trusted, &[]).unwrap();
  let second = read_file(&file, &b, &trusted, &[]).unwrap();

  assert_eq!(first.snapshot, second.snapshot);
}

#[test]
fn a_damaged_sync_file_is_refused() {
  let (a, b) = (identity("A"), identity("B"));
  let trusted = trusting(&[a.public_key]);
  let file = write_sync_file(&a, b.public_key, snapshot(&a, 2)).unwrap();

  let mut flipped = file.clone();
  let last = flipped.len() - 1;
  flipped[last] ^= 1;
  assert!(read_file(&flipped, &b, &trusted, &[]).is_err());

  let truncated = &file[..file.len() - 1];
  assert!(matches!(
    read_file(truncated, &b, &trusted, &[]),
    Err(FileError::Damaged)
  ));

  let mut extended = file;
  extended.extend_from_slice(b"\x00\x05extra");
  assert!(read_file(&extended, &b, &trusted, &[]).is_err());
}

#[test]
fn other_files_are_not_sync_files() {
  let me = identity("Me");

  for bytes in [
    &b""[..],
    b"ARS",
    b"VLT1\x00\x02whatever",
    b"ARSY\x00\x01\x07\x00\x01x",
  ] {
    assert!(
      matches!(
        read_file(bytes, &me, &DeviceSet::default(), &[]),
        Err(FileError::NotASyncFile)
      ),
      "{bytes:?}"
    );
  }
}

#[test]
fn a_newer_file_format_is_reported_as_such() {
  let me = identity("Me");

  assert!(matches!(
    read_file(b"ARSY\x00\x02\x01rest", &me, &DeviceSet::default(), &[]),
    Err(FileError::UnsupportedVersion(2))
  ));
}

#[test]
fn a_sync_file_relabelled_as_a_join_file_is_not_readable() {
  let (a, b) = (identity("A"), identity("B"));
  let mut file = write_sync_file(&a, b.public_key, snapshot(&a, 1)).unwrap();
  file[6] = 2;

  assert!(
    read_file(
      &file,
      &b,
      &trusting(&[a.public_key]),
      &[PairingSecret::from_bytes([0; 32])]
    )
    .is_err()
  );
}

// ---- join files -------------------------------------------------------------

fn qr_invitation(inviter: &DeviceIdentity, secret: [u8; 32]) -> Invitation {
  Invitation {
    secret: PairingSecret::from_bytes(secret),
    inviter: inviter.public_key,
  }
}

#[test]
fn a_join_file_introduces_its_sender_through_an_open_invitation() {
  let (phone_a, phone_b) = (identity("Phone A"), identity("Phone B"));
  let invitation = qr_invitation(&phone_a, [7; 32]);
  let sent = snapshot(&phone_b, 2);
  let file = write_join_file(&phone_b, &invitation, sent.clone()).unwrap();
  let open = [
    PairingSecret::from_bytes([1; 32]),
    invitation.secret.clone(),
  ];

  let read = read_file(&file, &phone_a, &trusting(&[phone_a.public_key]), &open).unwrap();

  assert_eq!(
    read.sender, phone_b.public_key,
    "not trusted yet, and accepted"
  );
  assert_eq!(read.snapshot, sent);
  assert_eq!(read.invitation, Some(invitation.secret));
}

#[test]
fn a_join_file_without_its_invitation_is_refused() {
  let (phone_a, phone_b) = (identity("Phone A"), identity("Phone B"));
  let file = write_join_file(
    &phone_b,
    &qr_invitation(&phone_a, [7; 32]),
    snapshot(&phone_b, 1),
  )
  .unwrap();

  assert!(matches!(
    read_file(
      &file,
      &phone_a,
      &DeviceSet::default(),
      &[PairingSecret::from_bytes([8; 32])]
    ),
    Err(FileError::NoInvitation)
  ));
  assert!(matches!(
    read_file(&file, &phone_a, &DeviceSet::default(), &[]),
    Err(FileError::NoInvitation)
  ));
}
