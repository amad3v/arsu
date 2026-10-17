use uuid::Uuid;
use vault_core::SyncState;

use super::*;
use crate::{devices, invitation};

const HERE: Uuid = Uuid::from_u128(1);
const ELSEWHERE: Uuid = Uuid::from_u128(2);

#[test]
fn the_first_sync_creates_an_identity() {
  let mut state = SyncState::default();

  let identity = reconcile(&mut state, HERE, "Computer").unwrap().clone();

  assert_eq!(identity.installation_id, HERE);
  assert_eq!(identity.name, "Computer");
  assert_eq!(state.identity.as_ref(), Some(&identity));
  assert!(
    state.devices.members.is_empty(),
    "an identity alone changes no device set, so it never fails on a full one"
  );
}

#[test]
fn the_same_installation_keeps_its_identity() {
  let mut state = SyncState::default();
  let first = reconcile(&mut state, HERE, "Computer").unwrap().clone();

  let again = reconcile(&mut state, HERE, "Renamed").unwrap();

  assert_eq!(*again, first);
}

#[test]
fn a_vault_copied_to_another_installation_gets_a_new_identity() {
  let mut state = SyncState::default();
  let original = reconcile(&mut state, HERE, "Computer").unwrap().clone();
  list_self(&mut state, 100).unwrap();
  invitation::open(&mut state, 100).unwrap();

  let copy = reconcile(&mut state, ELSEWHERE, "New computer")
    .unwrap()
    .clone();

  assert_ne!(copy.public_key, original.public_key);
  assert_eq!(copy.installation_id, ELSEWHERE);
  assert!(
    state.invitations.is_empty(),
    "the copied invitations belonged to the original device"
  );
  assert!(
    devices::is_trusted(&state.devices, &original.public_key),
    "the device set is kept: the original device is still a member"
  );
}

#[test]
fn listing_this_device_adds_its_record_once() {
  let mut state = SyncState::default();
  let identity = reconcile(&mut state, HERE, "Phone").unwrap().clone();

  list_self(&mut state, 100).unwrap();
  list_self(&mut state, 200).unwrap();

  assert_eq!(state.devices.members.len(), 1);
  assert_eq!(state.devices.members[0].public_key, identity.public_key);
  assert_eq!(state.devices.members[0].name, "Phone");
  assert_eq!(state.devices.members[0].added_at, 100);
}

#[test]
fn listing_this_device_needs_an_identity() {
  assert_eq!(
    list_self(&mut SyncState::default(), 1),
    Err(IdentityError::NoIdentity)
  );
}

#[test]
fn a_removed_device_forgets_its_identity() {
  let mut state = SyncState::default();
  let identity = reconcile(&mut state, HERE, "Phone").unwrap().clone();
  invitation::open(&mut state, 100).unwrap();
  devices::remove(&mut state.devices, identity.public_key, 150);

  assert!(forget_if_removed(&mut state));

  assert!(state.identity.is_none());
  assert!(state.invitations.is_empty());
  let fresh = reconcile(&mut state, HERE, "Phone").unwrap();
  assert_ne!(
    fresh.public_key, identity.public_key,
    "pairing anew means a new key"
  );
}

#[test]
fn a_device_still_trusted_keeps_its_identity() {
  let mut state = SyncState::default();
  reconcile(&mut state, HERE, "Phone").unwrap();

  assert!(!forget_if_removed(&mut state));
  assert!(state.identity.is_some());
  assert!(!forget_if_removed(&mut SyncState::default()));
}
