//! The merge rules, one test each, then the laws they
//! must obey together, checked on random states.

use otp::Algorithm;
use proptest::prelude::*;
use uuid::Uuid;
use vault_core::{Entry, OtpConfig, Tombstone, VaultPayload};

use super::*;

// ---- fixtures ------------------------------------------------------------

fn id(n: u128) -> Uuid {
  Uuid::from_u128(n)
}

fn totp(n: u128, label: &str, seed: &[u8]) -> Entry {
  Entry {
    id: id(n),
    issuer: Some("Example".to_string()),
    account_label: label.to_string(),
    otp: OtpConfig::Totp {
      secret: seed.to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      period: 30,
    },
    icon: None,
    tags: Vec::new(),
    notes: None,
    created_at: 100,
    updated_at: 100,
  }
}

fn hotp(n: u128, label: &str, seed: &[u8], counter: u64) -> Entry {
  Entry {
    otp: OtpConfig::Hotp {
      secret: seed.to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      counter,
    },
    ..totp(n, label, seed)
  }
}

fn hotp_counter_of(entry: &Entry) -> Option<u64> {
  match entry.otp {
    OtpConfig::Hotp { counter, .. } => Some(counter),
    OtpConfig::Totp { .. } => None,
  }
}

fn counter(entry: &Entry) -> u64 {
  match entry.otp {
    OtpConfig::Hotp { counter, .. } => counter,
    OtpConfig::Totp { .. } => panic!("not an HOTP entry"),
  }
}

fn replica(entries: Vec<Entry>, tombstones: Vec<Tombstone>) -> Replica {
  Replica {
    vault_id: id(1_000),
    entries,
    tombstones,
  }
}

fn tombstone(n: u128, deleted_at: u64) -> Tombstone {
  Tombstone::new(id(n), deleted_at)
}

fn ids(replica: &Replica) -> Vec<Uuid> {
  replica.entries.iter().map(|entry| entry.id).collect()
}

// ---- tombstones -----------------------------------------------------------

#[test]
fn tombstones_from_both_sides_are_kept() {
  let local = replica(vec![], vec![tombstone(1, 10)]);
  let remote = replica(vec![], vec![tombstone(2, 20)]);

  let (merged, _) = merge(&local, &remote);

  assert_eq!(merged.tombstones, vec![tombstone(1, 10), tombstone(2, 20)]);
}

#[test]
fn the_earliest_deletion_time_is_kept() {
  let local = replica(vec![], vec![tombstone(1, 30)]);
  let remote = replica(vec![], vec![tombstone(1, 20)]);

  assert_eq!(merge(&local, &remote).0.tombstones, vec![tombstone(1, 20)]);
  assert_eq!(merge(&remote, &local).0.tombstones, vec![tombstone(1, 20)]);
}

// ---- entries --------------------------------------------------------------

#[test]
fn an_entry_only_the_peer_has_is_added() {
  let local = replica(vec![totp(1, "alice", b"a")], vec![]);
  let remote = replica(vec![totp(2, "bob", b"b")], vec![]);

  let (merged, summary) = merge(&local, &remote);

  assert_eq!(ids(&merged), [id(1), id(2)]);
  assert_eq!(
    summary,
    MergeSummary {
      added: 1,
      ..MergeSummary::default()
    }
  );
}

#[test]
fn a_deletion_on_the_peer_deletes_the_entry_here() {
  let local = replica(vec![totp(1, "alice", b"a")], vec![]);
  let remote = replica(vec![], vec![tombstone(1, 50)]);

  let (merged, summary) = merge(&local, &remote);

  assert!(merged.entries.is_empty());
  assert_eq!(merged.tombstones, vec![tombstone(1, 50)]);
  assert_eq!(
    summary,
    MergeSummary {
      deleted: 1,
      ..MergeSummary::default()
    }
  );
}

#[test]
fn a_deletion_here_is_not_undone_by_a_peer_that_still_has_the_entry() {
  let local = replica(vec![], vec![tombstone(1, 50)]);
  let remote = replica(vec![totp(1, "alice", b"a")], vec![]);

  let (merged, summary) = merge(&local, &remote);

  assert!(merged.entries.is_empty(), "delete wins");
  assert_eq!(summary, MergeSummary::default());
}

#[test]
fn the_highest_hotp_counter_wins_whichever_side_has_it() {
  let low = replica(vec![hotp(1, "bank", b"s", 3)], vec![]);
  let high = replica(vec![hotp(1, "bank", b"s", 9)], vec![]);

  let (merged, summary) = merge(&low, &high);
  assert_eq!(counter(&merged.entries[0]), 9);
  assert_eq!(
    summary,
    MergeSummary {
      updated: 1,
      ..MergeSummary::default()
    }
  );

  let (merged, summary) = merge(&high, &low);
  assert_eq!(counter(&merged.entries[0]), 9);
  assert_eq!(summary, MergeSummary::default(), "nothing changed here");
}

#[test]
fn the_counter_never_goes_back_even_if_the_other_version_is_newer() {
  let advanced = hotp(1, "bank", b"s", 9);
  let renamed = Entry {
    account_label: "bank (renamed)".to_string(),
    updated_at: 500,
    ..hotp(1, "bank", b"s", 2)
  };

  let (merged, _) = merge(
    &replica(vec![advanced], vec![]),
    &replica(vec![renamed], vec![]),
  );

  assert_eq!(merged.entries[0].account_label, "bank (renamed)");
  assert_eq!(counter(&merged.entries[0]), 9);
}

#[test]
fn other_fields_come_from_the_most_recently_updated_version() {
  let old = totp(1, "old name", b"a");
  let new = Entry {
    account_label: "new name".to_string(),
    updated_at: 200,
    ..totp(1, "old name", b"a")
  };

  let (merged, summary) = merge(
    &replica(vec![old.clone()], vec![]),
    &replica(vec![new.clone()], vec![]),
  );
  assert_eq!(merged.entries[0].account_label, "new name");
  assert_eq!(summary.updated, 1);

  let (merged, _) = merge(&replica(vec![new], vec![]), &replica(vec![old], vec![]));
  assert_eq!(merged.entries[0].account_label, "new name");
}

#[test]
fn versions_updated_at_the_same_time_resolve_the_same_way_on_both_sides() {
  let a = replica(vec![totp(1, "one", b"a")], vec![]);
  let b = replica(vec![totp(1, "two", b"a")], vec![]);

  assert_eq!(merge(&a, &b).0, merge(&b, &a).0);
}

#[test]
fn the_earliest_creation_time_is_kept() {
  let early = Entry {
    created_at: 5,
    ..totp(1, "alice", b"a")
  };
  let late = Entry {
    created_at: 50,
    updated_at: 300,
    ..totp(1, "alice", b"a")
  };

  let (merged, _) = merge(&replica(vec![late], vec![]), &replica(vec![early], vec![]));

  assert_eq!(merged.entries[0].created_at, 5);
  assert_eq!(merged.entries[0].updated_at, 300);
}

#[test]
fn entries_are_ordered_by_id_which_is_creation_order() {
  let local = replica(vec![totp(3, "c", b"c"), totp(1, "a", b"a")], vec![]);
  let remote = replica(vec![totp(2, "b", b"b")], vec![]);

  assert_eq!(ids(&merge(&local, &remote).0), [id(1), id(2), id(3)]);
}

// ---- duplicates -------------------------------------------------------------

#[test]
fn the_same_account_added_on_two_devices_ends_up_once() {
  let local = replica(vec![totp(7, "alice", b"seed")], vec![]);
  let remote = replica(vec![totp(4, "Alice ", b"seed")], vec![]);

  let (merged, summary) = merge(&local, &remote);

  assert_eq!(ids(&merged), [id(4)], "the smallest id survives");
  assert_eq!(merged.tombstones.len(), 1);
  assert_eq!(merged.tombstones[0].id, id(7));
  assert_eq!(merge(&remote, &local).0, merged);
  assert_eq!(
    summary,
    MergeSummary::default(),
    "one copy replaced the other: no account was added or deleted"
  );
}

#[test]
fn a_collapsed_hotp_account_keeps_the_highest_counter() {
  let local = replica(vec![hotp(7, "bank", b"seed", 12)], vec![]);
  let remote = replica(vec![hotp(4, "bank", b"seed", 3)], vec![]);

  let (merged, _) = merge(&local, &remote);

  assert_eq!(ids(&merged), [id(4)]);
  assert_eq!(counter(&merged.entries[0]), 12);
}

#[test]
fn a_collapse_tombstone_names_the_surviving_copy() {
  let local = replica(vec![hotp(7, "bank", b"seed", 12)], vec![]);
  let remote = replica(vec![hotp(4, "bank", b"seed", 3)], vec![]);

  let (merged, _) = merge(&local, &remote);

  assert_eq!(
    merged.tombstones,
    vec![Tombstone {
      successor: Some(id(4)),
      hotp_counter: Some(12),
      ..tombstone(7, 100)
    }]
  );
}

#[test]
fn codes_shown_from_a_collapsed_copy_elsewhere_carry_over_to_the_survivor() {
  // Devices A and C added the same HOTP account separately.
  let a = replica(vec![hotp(1, "bank", b"s", 0)], vec![]);
  let c = replica(vec![hotp(2, "bank", b"s", 0)], vec![]);
  // A hears from C first, and collapses C's copy into its own.
  let a_after = merge(&a, &c).0;
  // Meanwhile C shows ten codes from its copy, then hears from A.
  let c_used = replica(vec![hotp(2, "bank", b"s", 10)], vec![]);
  let c_after = merge(&c_used, &a_after).0;

  assert_eq!(ids(&c_after), [id(1)]);
  assert_eq!(
    counter(&c_after.entries[0]),
    10,
    "the counter never goes back"
  );
  let a_final = merge(&a_after, &c_after).0;
  assert_eq!(counter(&a_final.entries[0]), 10);
}

#[test]
fn a_counter_follows_a_chain_of_collapses() {
  let older = replica(
    vec![hotp(1, "bank", b"s", 0)],
    vec![
      Tombstone {
        successor: Some(id(1)),
        ..tombstone(2, 50)
      },
      Tombstone {
        successor: Some(id(2)),
        ..tombstone(3, 60)
      },
    ],
  );
  let used = replica(vec![hotp(3, "bank", b"s", 8)], vec![]);

  let (merged, _) = merge(&older, &used);

  assert_eq!(ids(&merged), [id(1)]);
  assert_eq!(counter(&merged.entries[0]), 8);
}

#[test]
fn accounts_that_differ_in_secret_are_both_kept() {
  let local = replica(vec![totp(1, "alice", b"seed-1")], vec![]);
  let remote = replica(vec![totp(2, "alice", b"seed-2")], vec![]);

  assert_eq!(ids(&merge(&local, &remote).0), [id(1), id(2)]);
}

// ---- vault id ---------------------------------------------------------------

#[test]
fn both_sides_adopt_the_smaller_vault_id() {
  let local = Replica {
    vault_id: id(9),
    ..replica(vec![], vec![])
  };
  let remote = Replica {
    vault_id: id(3),
    ..replica(vec![], vec![])
  };

  assert_eq!(merge(&local, &remote).0.vault_id, id(3));
  assert_eq!(merge(&remote, &local).0.vault_id, id(3));
}

// ---- the payload ------------------------------------------------------------

#[test]
fn merging_an_identical_replica_changes_nothing() {
  let state = replica(
    vec![totp(1, "a", b"a"), hotp(2, "b", b"b", 4)],
    vec![tombstone(3, 10)],
  );

  assert_eq!(
    merge(&state, &state),
    (state.clone(), MergeSummary::default())
  );
}

#[test]
fn a_replica_reads_and_writes_the_payload() {
  let mut payload = VaultPayload::new();
  payload.entries.push(totp(1, "alice", b"a"));
  payload.tombstones.push(tombstone(2, 10));

  let mut state = Replica::of(&payload);
  assert_eq!(state.vault_id, payload.vault_id);
  assert_eq!(ids(&state), [id(1)]);
  assert_eq!(state.tombstones, vec![tombstone(2, 10)]);

  state.entries.push(totp(3, "bob", b"b"));
  state.store_in(&mut payload);
  assert_eq!(payload.entries.len(), 2);
}

// ---- laws -------------------------------------------------------------------

/// A small universe, so that random replicas share ids and accounts and
/// every rule — conflicts, deletions, duplicates — comes into play.
///
/// As in a real vault, an id always names the same account and scheme
/// (Arsu cannot edit an entry's secret or scheme); what varies between
/// versions of an id is its label (a rename), counter and timestamps.
/// Ids 0 and 3, 1 and 4, 2 and 5 are copies of the same account.
fn arb_entry() -> impl Strategy<Value = Entry> {
  (0..6u128, any::<bool>(), 0..4u64, 0..4u64, 0..3u64).prop_map(
    |(n, renamed, count, updated_at, created_at)| {
      let entry = match n % 3 {
        0 => totp(n, "alice", b"a"),
        1 => totp(n, "bob", b"b"),
        _ => hotp(n, "carol", b"c", count),
      };
      Entry {
        account_label: if renamed {
          format!("{} (renamed)", entry.account_label)
        } else {
          entry.account_label.clone()
        },
        updated_at,
        created_at,
        ..entry
      }
    },
  )
}

fn arb_replica() -> impl Strategy<Value = Replica> {
  (
    0..4u128,
    proptest::collection::vec(arb_entry(), 0..5),
    proptest::collection::vec((0..6u128, 0..4u64), 0..3),
  )
    .prop_map(|(vault, entries, tombstones)| {
      // A replica as a device holds it: one version per id, and no
      // live entry that is also a tombstone.
      let mut state = Replica {
        vault_id: id(vault),
        entries: Vec::new(),
        tombstones: Vec::new(),
      };
      for (n, deleted_at) in tombstones {
        if state.tombstones.iter().all(|t| t.id != id(n)) {
          state.tombstones.push(tombstone(n, deleted_at));
        }
      }
      for entry in entries {
        let taken = state.entries.iter().any(|e| e.id == entry.id)
          || state.tombstones.iter().any(|t| t.id == entry.id);
        if !taken {
          state.entries.push(entry);
        }
      }
      // Normalized the way a merge leaves it.
      merge(&state, &state).0
    })
}

proptest! {
  #[test]
  fn merge_is_commutative(a in arb_replica(), b in arb_replica()) {
    prop_assert_eq!(merge(&a, &b).0, merge(&b, &a).0);
  }

  #[test]
  fn merge_is_idempotent(a in arb_replica(), b in arb_replica()) {
    prop_assert_eq!(merge(&a, &a).0, a.clone());
    let ab = merge(&a, &b).0;
    prop_assert_eq!(merge(&ab, &b).0, ab.clone());
    prop_assert_eq!(merge(&ab, &a).0, ab);
  }

  /// Joining alone — without collapsing duplicates — forms a
  /// state-based CRDT.
  #[test]
  fn join_is_associative(a in arb_replica(), b in arb_replica(), c in arb_replica()) {
    prop_assert_eq!(join(&join(&a, &b), &c), join(&a, &join(&b, &c)));
  }

  /// Whatever the order devices sync in, once each has synced with
  /// every other (twice round, so changes travel through a third
  /// device), all hold the same state.
  #[test]
  fn devices_converge_whatever_the_sync_order(
    mut devices in proptest::collection::vec(arb_replica(), 2..5),
    syncs in proptest::collection::vec((0..4usize, 0..4usize), 0..8),
  ) {
    let count = devices.len();
    let sync = |devices: &mut Vec<Replica>, i: usize, j: usize| {
      let merged = merge(&devices[i], &devices[j]).0;
      devices[i] = merged.clone();
      devices[j] = merged;
    };
    for (i, j) in syncs {
      sync(&mut devices, i % count, j % count);
    }
    for _ in 0..2 {
      for i in 0..count {
        for j in 0..count {
          sync(&mut devices, i, j);
        }
      }
    }
    for device in &devices[1..] {
      prop_assert_eq!(device, &devices[0]);
    }
  }

  /// An HOTP counter never goes back: every live HOTP entry of either
  /// side is, after the merge, still live with at least its counter, or
  /// a tombstone whose chain of successors ends at such an entry, or
  /// deleted by the user (a chain that ends in no live entry).
  #[test]
  fn hotp_counters_never_go_back(a in arb_replica(), b in arb_replica()) {
    let merged = merge(&a, &b).0;
    for entry in a.entries.iter().chain(&b.entries) {
      let Some(used) = hotp_counter_of(entry) else { continue };
      let mut target = entry.id;
      loop {
        if let Some(live) = merged.entries.iter().find(|e| e.id == target) {
          prop_assert!(hotp_counter_of(live).unwrap_or(u64::MAX) >= used, "{:?} went back", entry.id);
          break;
        }
        match merged.tombstones.iter().find(|t| t.id == target).and_then(|t| t.successor) {
          Some(next) => target = next,
          None => break,
        }
      }
    }
  }

  /// No account is lost: after the join step, every live entry's account
  /// is still live once duplicates collapse — under its own id, or as the
  /// copy that survived. (A version superseded by a newer version of the
  /// same id is not an account of its own.)
  #[test]
  fn merge_loses_no_account(a in arb_replica(), b in arb_replica()) {
    let merged = merge(&a, &b).0;
    let live: Vec<vault_core::AccountKey> =
      merged.entries.iter().map(vault_core::AccountKey::from).collect();
    for entry in &join(&a, &b).entries {
      prop_assert!(
        live.contains(&vault_core::AccountKey::from(entry)),
        "lost {:?}", entry.id
      );
    }
  }
}
