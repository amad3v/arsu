//! How two copies of a vault merge.
//!
//! [`merge`] is two steps:
//!
//! 1. [`join`]: tombstones and entries are united,
//!    deletions win, HOTP counters take their maximum, other fields come
//!    from the most recently updated version, the smaller vault id wins.
//!    This step is a state-based CRDT — commutative, associative and
//!    idempotent — so devices may sync in any order, any number of times.
//! 2. [`collapse_duplicates`]: copies of one account added on
//!    different devices are reduced to one. It is a deterministic
//!    function of the joined state, so merges stay commutative and
//!    idempotent and devices still converge once all have synced.
//!
//! Merging assumes what Arsu guarantees: an entry's id always names the
//! same scheme (TOTP or HOTP), since entries cannot be edited.

use std::{
  cmp::Ordering,
  collections::{BTreeMap, HashMap, HashSet},
};

use uuid::Uuid;
use vault_core::{AccountKey, Entry, OtpConfig, Tombstone, VaultPayload, cbor};
use zeroize::Zeroizing;

/// The part of a vault that sync exchanges and merges: its id, its live
/// entries and its tombstones.
///
/// A merged replica lists entries and tombstones by id. Ids are `UUIDv7`,
/// which start with their creation time, so this is the order the
/// accounts were added in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replica {
  pub vault_id: Uuid,
  pub entries: Vec<Entry>,
  pub tombstones: Vec<Tombstone>,
}

/// What a merge changed, from the local device's point of view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeSummary {
  /// Accounts that were not here and now are.
  pub added: usize,
  /// Accounts that were here and changed (an HOTP counter moved on).
  pub updated: usize,
  /// Accounts that were here and are now deleted.
  pub deleted: usize,
}

impl Replica {
  /// A copy of the payload's sync-relevant parts.
  #[must_use]
  pub fn of(payload: &VaultPayload) -> Self {
    Self {
      vault_id: payload.vault_id,
      entries: payload.entries.clone(),
      tombstones: payload.tombstones.clone(),
    }
  }

  /// Replaces the payload's sync-relevant parts with this replica's.
  pub fn store_in(self, payload: &mut VaultPayload) {
    payload.vault_id = self.vault_id;
    payload.entries = self.entries;
    payload.tombstones = self.tombstones;
  }
}

/// Merges a peer's replica into the local one: the new local state, and
/// what changed compared with `local`.
#[must_use]
pub fn merge(local: &Replica, remote: &Replica) -> (Replica, MergeSummary) {
  let merged = collapse_duplicates(join(local, remote));
  let summary = MergeSummary::between(local, &merged);
  (merged, summary)
}

/// The first step of [`merge`]: unites tombstones, entries and vault ids.
pub(crate) fn join(a: &Replica, b: &Replica) -> Replica {
  let mut tombstones = BTreeMap::new();
  for tombstone in a.tombstones.iter().chain(&b.tombstones) {
    add_tombstone(&mut tombstones, *tombstone);
  }

  let mut entries: BTreeMap<Uuid, Entry> = BTreeMap::new();
  for entry in a.entries.iter().chain(&b.entries) {
    if let Some(tombstone) = tombstones.get_mut(&entry.id) {
      // Delete wins; the counter this version reached lives on in the
      // tombstone, for its successor.
      tombstone.hotp_counter = max_counter(tombstone.hotp_counter, hotp_counter(entry));
      continue;
    }
    entries
      .entry(entry.id)
      .and_modify(|known| *known = combine(known, entry))
      .or_insert_with(|| entry.clone());
  }
  carry_counters(&mut entries, &mut tombstones);

  Replica {
    vault_id: a.vault_id.min(b.vault_id),
    entries: entries.into_values().collect(),
    tombstones: tombstones.into_values().collect(),
  }
}

/// The second step of [`merge`]: of live entries that are the same account (see
/// [`AccountKey`]), the one with the smallest id survives, with the
/// group's highest HOTP counter; the others become tombstones naming it
/// as their successor, dated by their own last update so that every
/// device dates them alike.
pub(crate) fn collapse_duplicates(replica: Replica) -> Replica {
  let mut survivors: HashMap<AccountKey, usize> = HashMap::new();
  let mut entries: Vec<Entry> = Vec::with_capacity(replica.entries.len());
  let mut tombstones: BTreeMap<Uuid, Tombstone> = replica
    .tombstones
    .into_iter()
    .map(|tombstone| (tombstone.id, tombstone))
    .collect();

  // Entries are sorted by id, so the first of each account survives.
  for entry in replica.entries {
    let key = AccountKey::from(&entry);
    if let Some(&survivor) = survivors.get(&key) {
      let survivor = &mut entries[survivor];
      raise_counter(survivor, hotp_counter(&entry));
      add_tombstone(
        &mut tombstones,
        Tombstone {
          id: entry.id,
          deleted_at: entry.updated_at,
          successor: Some(survivor.id),
          hotp_counter: hotp_counter(&entry),
        },
      );
    } else {
      survivors.insert(key, entries.len());
      entries.push(entry);
    }
  }

  Replica {
    vault_id: replica.vault_id,
    entries,
    tombstones: tombstones.into_values().collect(),
  }
}

/// Carries HOTP counters down chains of successors: every tombstone and
/// the live entry a chain leads to are raised to the counters of the
/// tombstones above them, so that codes shown from a copy collapsed
/// elsewhere are never shown again — even when the survivor has itself
/// been merged or deleted since.
fn carry_counters(entries: &mut BTreeMap<Uuid, Entry>, tombstones: &mut BTreeMap<Uuid, Tombstone>) {
  // Highest ids first: a successor always has a smaller id, so each
  // tombstone has received everything from above before passing it on.
  let ids: Vec<Uuid> = tombstones.keys().rev().copied().collect();
  for id in ids {
    let Some(&Tombstone {
      successor: Some(successor),
      hotp_counter: Some(counter),
      ..
    }) = tombstones.get(&id)
    else {
      continue;
    };
    if successor >= id {
      // Not a chain the merge makes: only a corrupt peer sends one.
      continue;
    }
    if let Some(next) = tombstones.get_mut(&successor) {
      next.hotp_counter = max_counter(next.hotp_counter, Some(counter));
    } else if let Some(entry) = entries.get_mut(&successor) {
      raise_counter(entry, Some(counter));
    }
  }
}

/// Follows a tombstone's chain of successors to the first id `is_live`
/// accepts, if any. A survivor always has a smaller id than the copies
/// merged into it, so a chain that does not strictly decrease — which
/// only a corrupt or hostile peer could send — is not followed.
fn successor_of(
  tombstone: &Tombstone,
  tombstones: &BTreeMap<Uuid, Tombstone>,
  is_live: impl Fn(Uuid) -> bool,
) -> Option<Uuid> {
  let mut current = tombstone.id;
  let mut next = tombstone.successor;
  while let Some(id) = next {
    if id >= current {
      return None;
    }
    if is_live(id) {
      return Some(id);
    }
    current = id;
    next = tombstones
      .get(&id)
      .and_then(|tombstone| tombstone.successor);
  }
  None
}

/// Two versions of one entry, as one.
fn combine(a: &Entry, b: &Entry) -> Entry {
  let mut combined = match compare_versions(a, b) {
    Ordering::Less => b.clone(),
    Ordering::Equal | Ordering::Greater => a.clone(),
  };
  combined.created_at = a.created_at.min(b.created_at);
  raise_counter(&mut combined, hotp_counter(a));
  raise_counter(&mut combined, hotp_counter(b));
  combined
}

/// Which version's fields win: the later `updated_at`, then — so that
/// every device picks the same one — the greater canonical encoding.
/// The fields merged on their own (HOTP counter, `created_at`) are left
/// out, so the comparison does not change once they are merged.
fn compare_versions(a: &Entry, b: &Entry) -> Ordering {
  a.updated_at
    .cmp(&b.updated_at)
    .then_with(|| canonical_fields(a).cmp(&canonical_fields(b)))
}

/// The entry's CBOR encoding, without the separately merged fields. It
/// holds the seed, so the buffer is zeroized when dropped.
fn canonical_fields(entry: &Entry) -> Zeroizing<Vec<u8>> {
  let mut fields = entry.clone();
  fields.created_at = 0;
  if let OtpConfig::Hotp { counter, .. } = &mut fields.otp {
    *counter = 0;
  }
  // Encoding these plain derived types into memory cannot fail.
  cbor::to_zeroizing_vec(&fields).expect("an entry always encodes as CBOR")
}

fn hotp_counter(entry: &Entry) -> Option<u64> {
  match entry.otp {
    OtpConfig::Hotp { counter, .. } => Some(counter),
    OtpConfig::Totp { .. } => None,
  }
}

/// Moves an HOTP entry's counter up to `seen`; never down.
fn raise_counter(entry: &mut Entry, seen: Option<u64>) {
  if let (OtpConfig::Hotp { counter, .. }, Some(seen)) = (&mut entry.otp, seen) {
    *counter = (*counter).max(seen);
  }
}

/// Adds a tombstone. For an id already there: the earliest deletion
/// time, the smallest successor, the highest counter — each a join, so
/// tombstones merge in any order.
fn add_tombstone(tombstones: &mut BTreeMap<Uuid, Tombstone>, tombstone: Tombstone) {
  tombstones
    .entry(tombstone.id)
    .and_modify(|known| {
      known.deleted_at = known.deleted_at.min(tombstone.deleted_at);
      known.successor = match (known.successor, tombstone.successor) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (one, other) => one.or(other),
      };
      known.hotp_counter = max_counter(known.hotp_counter, tombstone.hotp_counter);
    })
    .or_insert(tombstone);
}

fn max_counter(a: Option<u64>, b: Option<u64>) -> Option<u64> {
  a.max(b)
}

impl MergeSummary {
  /// Compares the live entries before and after a merge. An entry merged
  /// into another copy of its account (a tombstone whose successors lead
  /// to a live entry) is neither deleted nor, its successor, added: the
  /// account is still there.
  fn between(before: &Replica, after: &Replica) -> Self {
    let before_entries: HashMap<Uuid, &Entry> = before.entries.iter().map(|e| (e.id, e)).collect();
    let after_ids: HashSet<Uuid> = after.entries.iter().map(|entry| entry.id).collect();
    let tombstones: BTreeMap<Uuid, Tombstone> = after
      .tombstones
      .iter()
      .map(|tombstone| (tombstone.id, *tombstone))
      .collect();

    let mut summary = Self::default();
    let mut continued: HashSet<Uuid> = HashSet::new();
    for id in before_entries.keys().filter(|id| !after_ids.contains(id)) {
      let survivor = tombstones
        .get(id)
        .and_then(|tombstone| successor_of(tombstone, &tombstones, |id| after_ids.contains(&id)));
      match survivor {
        Some(survivor) => {
          continued.insert(survivor);
        }
        None => summary.deleted += 1,
      }
    }
    for entry in &after.entries {
      match before_entries.get(&entry.id) {
        Some(old) if *old != entry => summary.updated += 1,
        Some(_) => {}
        None if continued.contains(&entry.id) => {}
        None => summary.added += 1,
      }
    }
    summary
  }
}

#[cfg(test)]
mod tests;
