use proptest::prelude::*;
use vault_core::{DeviceSet, Member, PUBLIC_KEY_LEN, PublicKey, Removal};

use super::*;

fn key(n: u8) -> PublicKey {
  PublicKey([n; PUBLIC_KEY_LEN])
}

fn member(n: u8, name: &str, added_at: u64) -> Member {
  Member {
    public_key: key(n),
    name: name.to_string(),
    added_at,
  }
}

fn set_of(members: &[Member]) -> DeviceSet {
  let mut set = DeviceSet::default();
  for member in members {
    add(&mut set, member.clone()).unwrap();
  }
  set
}

fn trusted_keys(set: &DeviceSet) -> Vec<PublicKey> {
  trusted(set).map(|member| member.public_key).collect()
}

#[test]
fn an_added_device_is_trusted() {
  let set = set_of(&[member(1, "Computer", 10)]);

  assert!(is_trusted(&set, &key(1)));
  assert!(!is_trusted(&set, &key(2)));
}

#[test]
fn a_removed_device_is_not_trusted() {
  let mut set = set_of(&[member(1, "Computer", 10), member(2, "Phone", 10)]);

  remove(&mut set, key(2), 20);

  assert!(!is_trusted(&set, &key(2)));
  assert_eq!(trusted_keys(&set), [key(1)]);
}

#[test]
fn a_removed_device_cannot_be_added_again() {
  let mut set = set_of(&[member(1, "Phone", 10)]);
  remove(&mut set, key(1), 20);

  assert_eq!(
    add(&mut set, member(1, "Phone", 30)),
    Err(DeviceSetError::Removed)
  );
  assert!(!is_trusted(&set, &key(1)));
}

#[test]
fn a_removal_wins_over_a_peer_that_still_lists_the_device() {
  let mut here = set_of(&[member(1, "Computer", 10), member(2, "Phone", 10)]);
  remove(&mut here, key(2), 20);
  let peer = set_of(&[member(1, "Computer", 10), member(2, "Phone", 10)]);

  let merged = merge(&here, &peer).unwrap();

  assert!(!is_trusted(&merged, &key(2)));
  assert_eq!(merge(&peer, &here).unwrap(), merged);
}

#[test]
fn a_device_can_be_removed_before_it_is_known() {
  let mut set = DeviceSet::default();
  remove(&mut set, key(5), 20);

  let merged = merge(&set, &set_of(&[member(5, "Phone", 10)])).unwrap();

  assert!(!is_trusted(&merged, &key(5)));
}

#[test]
fn merging_unites_both_sets() {
  let a = set_of(&[member(1, "Computer", 10)]);
  let b = set_of(&[member(2, "Phone", 20)]);

  assert_eq!(trusted_keys(&merge(&a, &b).unwrap()), [key(1), key(2)]);
}

#[test]
fn the_earliest_record_of_a_device_wins() {
  let a = set_of(&[member(1, "Old name", 10)]);
  let b = set_of(&[member(1, "New name", 30)]);

  let merged = merge(&a, &b).unwrap();
  assert_eq!(merged.members, vec![member(1, "Old name", 10)]);
  assert_eq!(merge(&b, &a).unwrap(), merged);
}

#[test]
fn records_from_the_same_time_resolve_the_same_way_on_both_sides() {
  let a = set_of(&[member(1, "Alpha", 10)]);
  let b = set_of(&[member(1, "Beta", 10)]);

  assert_eq!(merge(&a, &b).unwrap(), merge(&b, &a).unwrap());
}

#[test]
fn the_earliest_removal_time_is_kept() {
  let mut a = DeviceSet::default();
  remove(&mut a, key(1), 30);
  let mut b = DeviceSet::default();
  remove(&mut b, key(1), 20);

  assert_eq!(
    merge(&a, &b).unwrap().removed,
    vec![Removal {
      public_key: key(1),
      removed_at: 20
    }]
  );
}

#[test]
fn removing_twice_keeps_the_first_time() {
  let mut set = DeviceSet::default();
  remove(&mut set, key(1), 20);
  remove(&mut set, key(1), 40);

  assert_eq!(set.removed.len(), 1);
  assert_eq!(set.removed[0].removed_at, 20);
}

#[test]
fn adding_a_device_twice_keeps_one_record() {
  let mut set = set_of(&[member(1, "Computer", 10)]);
  add(&mut set, member(1, "Computer", 10)).unwrap();

  assert_eq!(set.members.len(), 1);
}

#[test]
fn no_more_than_the_maximum_of_devices_can_be_trusted() {
  let members: Vec<Member> = (0..MAX_TRUSTED_DEVICES)
    .map(|n| member(u8::try_from(n).unwrap(), "Device", 1))
    .collect();
  let mut set = set_of(&members);

  assert_eq!(
    add(&mut set, member(200, "One too many", 1)),
    Err(DeviceSetError::TooManyDevices)
  );
  assert_eq!(
    merge(&set, &set_of(&[member(201, "One too many", 1)])),
    Err(DeviceSetError::TooManyDevices)
  );
}

#[test]
fn removed_devices_free_their_place() {
  let members: Vec<Member> = (0..MAX_TRUSTED_DEVICES)
    .map(|n| member(u8::try_from(n).unwrap(), "Device", 1))
    .collect();
  let mut set = set_of(&members);
  remove(&mut set, key(0), 2);

  assert_eq!(add(&mut set, member(200, "Replacement", 3)), Ok(()));
}

fn arb_set() -> impl Strategy<Value = DeviceSet> {
  (
    proptest::collection::vec((0..6u8, 0..2usize, 0..3u64), 0..5),
    proptest::collection::vec((0..6u8, 0..3u64), 0..3),
  )
    .prop_map(|(members, removals)| {
      let mut set = DeviceSet::default();
      for (n, name, added_at) in members {
        let _ = add(&mut set, member(n, ["A", "B"][name], added_at));
      }
      for (n, removed_at) in removals {
        remove(&mut set, key(n), removed_at);
      }
      set
    })
}

proptest! {
  #[test]
  fn merge_is_commutative(a in arb_set(), b in arb_set()) {
    prop_assert_eq!(merge(&a, &b), merge(&b, &a));
  }

  #[test]
  fn merge_is_associative(a in arb_set(), b in arb_set(), c in arb_set()) {
    let left = merge(&merge(&a, &b).unwrap(), &c).unwrap();
    let right = merge(&a, &merge(&b, &c).unwrap()).unwrap();
    prop_assert_eq!(left, right);
  }

  #[test]
  fn merge_is_idempotent(a in arb_set(), b in arb_set()) {
    prop_assert_eq!(merge(&a, &a).unwrap(), a.clone());
    let ab = merge(&a, &b).unwrap();
    prop_assert_eq!(merge(&ab, &b).unwrap(), ab);
  }

  #[test]
  fn a_removed_key_stays_removed_through_any_merge(a in arb_set(), b in arb_set()) {
    let merged = merge(&a, &b).unwrap();
    for removal in a.removed.iter().chain(&b.removed) {
      prop_assert!(!is_trusted(&merged, &removal.public_key));
    }
  }
}
