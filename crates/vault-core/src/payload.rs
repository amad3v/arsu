//! The decrypted contents of a vault.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use otp::Algorithm;

use crate::{SecretBytes, SyncState, VaultCoreError, cbor};

/// The decrypted contents of a vault: everything inside the ciphertext.
///
/// # Compatibility
///
/// Up to Arsu 1.1, a deleted entry stayed in `entries` with its
/// `deleted_at` set — seed, label and all. Such payloads are still read:
/// every entry marked deleted becomes a [`Tombstone`] and its contents
/// are dropped, so the next save no longer holds them.
///
/// The other way round, an older build reads this payload too: it finds
/// only live entries (a missing `deleted_at` reads as "not deleted") and
/// ignores the `tombstones` list and the sync state, which it then drops
/// when it saves (the device must then be paired again).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "LegacyPayload")]
pub struct VaultPayload {
  /// Stable identity for this vault. Sync uses it to tell "two copies of
  /// the same vault" apart from "two different vaults that happen to
  /// have similar entries".
  pub vault_id: Uuid,
  /// The live accounts, in the order they were added.
  pub entries: Vec<Entry>,
  /// What is left of deleted entries: their id and when they were
  /// deleted, nothing else. Kept so that sync can tell "deleted here"
  /// apart from "never seen here" and does not bring a deleted account
  /// back from another device.
  pub tombstones: Vec<Tombstone>,
  /// This device's sync identity and the devices it trusts.
  pub sync: SyncState,
}

/// A deleted entry, reduced to what sync needs: which entry, and when it
/// was deleted. It deliberately holds no seed, label or note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tombstone {
  pub id: Uuid,
  /// Unix seconds.
  pub deleted_at: u64,
  /// When sync merged this entry into another copy of the same account:
  /// that copy. `None` for an entry the user deleted.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub successor: Option<Uuid>,
  /// The highest HOTP counter seen for this entry, carried over to its
  /// successor so that a counter never goes back.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub hotp_counter: Option<u64>,
}

impl Tombstone {
  /// The tombstone of an entry the user deleted at `deleted_at`.
  #[must_use]
  pub fn new(id: Uuid, deleted_at: u64) -> Self {
    Self {
      id,
      deleted_at,
      successor: None,
      hotp_counter: None,
    }
  }
}

impl VaultPayload {
  #[must_use]
  pub fn new() -> Self {
    Self {
      vault_id: Uuid::now_v7(),
      entries: Vec::new(),
      tombstones: Vec::new(),
      sync: SyncState::default(),
    }
  }

  /// Serialize to CBOR — the plaintext that gets encrypted — in a
  /// buffer allocated once and zeroized on drop (see [`cbor`]).
  ///
  /// # Errors
  ///
  /// Returns [`VaultCoreError::Encode`] if the payload cannot be
  /// serialized as CBOR.
  pub fn to_bytes(&self) -> Result<Zeroizing<Vec<u8>>, VaultCoreError> {
    cbor::to_zeroizing_vec(self)
  }

  /// Parse decrypted CBOR, staging strings — seeds included — in a
  /// buffer zeroized afterwards (see [`cbor`]).
  ///
  /// # Errors
  ///
  /// Returns [`VaultCoreError::Decode`] if the bytes cannot be
  /// deserialized as a vault payload.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, VaultCoreError> {
    cbor::from_slice(bytes)
  }

  /// The live entry with this id.
  #[must_use]
  pub fn entry(&self, id: Uuid) -> Option<&Entry> {
    self.entries.iter().find(|entry| entry.id == id)
  }

  /// The live entry with this id, to change it.
  pub fn entry_mut(&mut self, id: Uuid) -> Option<&mut Entry> {
    self.entries.iter_mut().find(|entry| entry.id == id)
  }

  /// Deletes the live entry with this id: it is replaced by a
  /// [`Tombstone`] recording `now` (Unix seconds), and its contents are
  /// dropped (the seed is zeroized). Returns whether there was such an
  /// entry; if not, nothing changes.
  pub fn delete_entry(&mut self, id: Uuid, now: u64) -> bool {
    let Some(position) = self.entries.iter().position(|entry| entry.id == id) else {
      return false;
    };
    self.entries.remove(position);
    self.tombstones.push(Tombstone::new(id, now));
    true
  }
}

impl Default for VaultPayload {
  fn default() -> Self {
    Self::new()
  }
}

/// One account. Equality compares every field (the seed in constant
/// time, see [`SecretBytes`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
  pub id: Uuid,
  /// `None` when the account genuinely has no issuer — not every
  /// service sets one, and forcing an empty-string sentinel instead
  /// of modeling that properly is exactly the kind of thing that
  /// causes display bugs later ("" vs "Unknown" vs blank).
  pub issuer: Option<String>,
  pub account_label: String,
  pub otp: OtpConfig,
  /// Icon representation is still an open design question — bundled
  /// vs. fetched. Kept as a bare
  /// identifier string for now so this struct doesn't lock in an
  /// answer that hasn't been made yet.
  pub icon: Option<String>,
  pub tags: Vec<String>,
  pub notes: Option<String>,
  /// Unix seconds. No calendar/timezone handling lives in this crate —
  /// there's nothing here that needs it, so no date/time crate
  /// dependency either.
  pub created_at: u64,
  pub updated_at: u64,
}

/// How a payload is read: [`VaultPayload`]'s fields, plus the shape Arsu
/// 1.1 and older wrote (see [`VaultPayload`]'s compatibility notes).
#[derive(Deserialize)]
struct LegacyPayload {
  vault_id: Uuid,
  entries: Vec<LegacyEntry>,
  #[serde(default)]
  tombstones: Vec<Tombstone>,
  #[serde(default)]
  sync: SyncState,
}

/// An [`Entry`] as read, with the `deleted_at` that older payloads may
/// carry. Its fields are spelled out rather than flattened from
/// [`Entry`]: serde's `flatten` would stage the whole entry, seed
/// included, in an intermediate buffer that is never zeroized.
#[derive(Deserialize)]
struct LegacyEntry {
  id: Uuid,
  issuer: Option<String>,
  account_label: String,
  otp: OtpConfig,
  icon: Option<String>,
  tags: Vec<String>,
  notes: Option<String>,
  created_at: u64,
  updated_at: u64,
  #[serde(default)]
  deleted_at: Option<u64>,
}

impl From<LegacyPayload> for VaultPayload {
  fn from(legacy: LegacyPayload) -> Self {
    let mut payload = Self {
      vault_id: legacy.vault_id,
      entries: Vec::with_capacity(legacy.entries.len()),
      tombstones: legacy.tombstones,
      sync: legacy.sync,
    };
    for entry in legacy.entries {
      match entry.deleted_at {
        // Dropping `entry` here zeroizes its seed.
        Some(deleted_at) => payload
          .tombstones
          .push(Tombstone::new(entry.id, deleted_at)),
        None => payload.entries.push(Entry {
          id: entry.id,
          issuer: entry.issuer,
          account_label: entry.account_label,
          otp: entry.otp,
          icon: entry.icon,
          tags: entry.tags,
          notes: entry.notes,
          created_at: entry.created_at,
          updated_at: entry.updated_at,
        }),
      }
    }
    payload
  }
}

/// An entry's OTP parameters. Deriving `Debug` is safe: the seed is a
/// [`SecretBytes`], whose `Debug` is redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OtpConfig {
  Totp {
    secret: SecretBytes,
    algorithm: Algorithm,
    digits: u8,
    period: u64,
  },
  Hotp {
    secret: SecretBytes,
    algorithm: Algorithm,
    digits: u8,
    counter: u64,
  },
}

#[cfg(test)]
mod tests {
  use super::*;

  const SEED: &[u8] = b"12345678901234567890";

  fn sample_entry() -> Entry {
    Entry {
      id: Uuid::now_v7(),
      issuer: Some("Example Co".to_string()),
      account_label: "alice@example.com".to_string(),
      otp: OtpConfig::Totp {
        secret: SEED.to_vec().into(),
        algorithm: Algorithm::Sha1,
        digits: 6,
        period: 30,
      },
      icon: None,
      tags: vec!["work".to_string()],
      notes: None,
      created_at: 1_700_000_000,
      updated_at: 1_700_000_000,
    }
  }

  #[test]
  fn payload_round_trips_through_cbor() {
    let mut payload = VaultPayload::new();
    payload.entries.push(sample_entry());

    let bytes = payload.to_bytes().unwrap();
    let decoded = VaultPayload::from_bytes(&bytes).unwrap();

    assert_eq!(decoded.vault_id, payload.vault_id);
    assert_eq!(decoded.entries.len(), 1);
    assert_eq!(decoded.entries[0].issuer.as_deref(), Some("Example Co"));
    match &decoded.entries[0].otp {
      OtpConfig::Totp {
        secret,
        digits,
        period,
        ..
      } => {
        assert_eq!(secret.expose_secret(), SEED);
        assert_eq!(*digits, 6);
        assert_eq!(*period, 30);
      }
      OtpConfig::Hotp { .. } => panic!("expected Totp"),
    }
  }

  #[test]
  fn hotp_entry_round_trips_through_cbor() {
    let mut payload = VaultPayload::new();
    payload.entries.push(Entry {
      otp: OtpConfig::Hotp {
        secret: SEED.to_vec().into(),
        algorithm: Algorithm::Sha256,
        digits: 8,
        counter: 42,
      },
      ..sample_entry()
    });

    let bytes = payload.to_bytes().unwrap();
    let decoded = VaultPayload::from_bytes(&bytes).unwrap();

    assert_eq!(decoded.entries[0].otp, payload.entries[0].otp);
  }

  #[test]
  fn serialized_plaintext_is_allocated_exactly_once() {
    let mut payload = VaultPayload::new();
    for _ in 0..50 {
      payload.entries.push(sample_entry());
    }

    let bytes = payload.to_bytes().unwrap();
    assert_eq!(
      bytes.capacity(),
      bytes.len(),
      "a buffer that grew would have left plaintext copies behind"
    );
  }

  #[test]
  fn debug_output_never_contains_a_seed() {
    let mut payload = VaultPayload::new();
    payload.entries.push(sample_entry());

    let printed = format!("{payload:?}");
    assert!(printed.contains("SecretBytes([REDACTED])"));
    assert!(!printed.contains("12345678901234567890"));
    assert!(!printed.contains("49, 50, 51"), "seed bytes as decimals");
  }

  #[test]
  fn deleting_an_entry_leaves_only_a_tombstone() {
    let mut payload = VaultPayload::new();
    let keep = sample_entry();
    let gone = sample_entry();
    let gone_id = gone.id;
    payload.entries.push(keep.clone());
    payload.entries.push(gone);

    assert!(payload.delete_entry(gone_id, 1_700_000_500));

    assert_eq!(payload.entries.len(), 1, "the deleted entry is removed");
    assert_eq!(payload.entries[0].id, keep.id);
    assert_eq!(
      payload.tombstones,
      vec![Tombstone::new(gone_id, 1_700_000_500)]
    );
  }

  #[test]
  fn a_tombstone_carries_no_secret_or_label() {
    let mut payload = VaultPayload::new();
    let entry = sample_entry();
    let id = entry.id;
    payload.entries.push(entry);
    payload.delete_entry(id, 1_700_000_500);

    let bytes = payload.to_bytes().unwrap();
    assert!(
      !contains(&bytes, SEED),
      "the deleted seed must not survive in the plaintext"
    );
    assert!(!contains(&bytes, b"alice@example.com"));
  }

  #[test]
  fn deleting_an_unknown_entry_changes_nothing() {
    let mut payload = VaultPayload::new();
    payload.entries.push(sample_entry());

    assert!(!payload.delete_entry(Uuid::now_v7(), 1_700_000_500));
    assert_eq!(payload.entries.len(), 1);
    assert!(payload.tombstones.is_empty());
  }

  #[test]
  fn tombstones_round_trip_through_cbor() {
    let mut payload = VaultPayload::new();
    let entry = sample_entry();
    let id = entry.id;
    payload.entries.push(entry);
    payload.delete_entry(id, 1_700_000_500);

    let decoded = VaultPayload::from_bytes(&payload.to_bytes().unwrap()).unwrap();
    assert!(decoded.entries.is_empty());
    assert_eq!(decoded.tombstones, payload.tombstones);
  }

  #[test]
  fn entry_lookup_finds_live_entries_only() {
    let mut payload = VaultPayload::new();
    let live = sample_entry();
    let dead = sample_entry();
    let (live_id, dead_id) = (live.id, dead.id);
    payload.entries.extend([live, dead]);
    payload.delete_entry(dead_id, 1);

    assert_eq!(payload.entry(live_id).map(|entry| entry.id), Some(live_id));
    assert!(payload.entry(dead_id).is_none());
    assert!(payload.entry_mut(dead_id).is_none());
  }

  /// The payload as format versions up to 1.1 wrote it: tombstones were
  /// entries with `deleted_at` set, and there was no `tombstones` list.
  mod legacy {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    use crate::OtpConfig;

    #[derive(Serialize, Deserialize)]
    pub struct Payload {
      pub vault_id: Uuid,
      pub entries: Vec<Entry>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Entry {
      pub id: Uuid,
      pub issuer: Option<String>,
      pub account_label: String,
      pub otp: OtpConfig,
      pub icon: Option<String>,
      pub tags: Vec<String>,
      pub notes: Option<String>,
      pub created_at: u64,
      pub updated_at: u64,
      pub deleted_at: Option<u64>,
    }

    impl From<super::Entry> for Entry {
      fn from(entry: super::Entry) -> Self {
        Self {
          id: entry.id,
          issuer: entry.issuer,
          account_label: entry.account_label,
          otp: entry.otp,
          icon: entry.icon,
          tags: entry.tags,
          notes: entry.notes,
          created_at: entry.created_at,
          updated_at: entry.updated_at,
          deleted_at: None,
        }
      }
    }
  }

  fn legacy_bytes(payload: &legacy::Payload) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(payload, &mut out).unwrap();
    out
  }

  #[test]
  fn legacy_tombstoned_entries_become_tombstones_when_read() {
    let live = sample_entry();
    let live_id = live.id;
    let mut dead = legacy::Entry::from(sample_entry());
    dead.deleted_at = Some(1_700_000_500);
    let dead_id = dead.id;
    let old = legacy::Payload {
      vault_id: Uuid::now_v7(),
      entries: vec![live.into(), dead],
    };

    let decoded = VaultPayload::from_bytes(&legacy_bytes(&old)).unwrap();

    assert_eq!(decoded.vault_id, old.vault_id);
    assert_eq!(decoded.entries.len(), 1);
    assert_eq!(decoded.entries[0].id, live_id);
    assert_eq!(
      decoded.tombstones,
      vec![Tombstone::new(dead_id, 1_700_000_500)]
    );
  }

  #[test]
  fn migrating_a_legacy_tombstone_drops_its_secret() {
    let mut dead = legacy::Entry::from(Entry {
      account_label: "bob@example.com".to_string(),
      otp: OtpConfig::Totp {
        secret: b"ZZZZZZZZZZZZZZZZZZZZ".to_vec().into(),
        algorithm: Algorithm::Sha1,
        digits: 6,
        period: 30,
      },
      ..sample_entry()
    });
    dead.deleted_at = Some(1);
    let old = legacy::Payload {
      vault_id: Uuid::now_v7(),
      entries: vec![dead],
    };

    let decoded = VaultPayload::from_bytes(&legacy_bytes(&old)).unwrap();
    let reencoded = decoded.to_bytes().unwrap();

    assert!(!contains(&reencoded, b"ZZZZZZZZZZZZZZZZZZZZ"));
    assert!(!contains(&reencoded, b"bob@example.com"));
  }

  #[test]
  fn older_builds_still_read_the_new_payload_without_resurrecting_deletions() {
    let mut payload = VaultPayload::new();
    let live = sample_entry();
    let dead = sample_entry();
    let (live_id, dead_id) = (live.id, dead.id);
    payload.entries.extend([live, dead]);
    payload.delete_entry(dead_id, 1);

    let bytes = payload.to_bytes().unwrap();
    let old: legacy::Payload = ciborium::from_reader(bytes.as_slice()).unwrap();

    assert_eq!(old.entries.len(), 1);
    assert_eq!(old.entries[0].id, live_id);
    assert_eq!(old.entries[0].deleted_at, None);
  }

  #[test]
  fn sync_state_round_trips_with_the_payload() {
    let mut payload = VaultPayload::new();
    payload.sync.devices.members.push(crate::Member {
      public_key: crate::PublicKey([3; crate::PUBLIC_KEY_LEN]),
      name: "Phone".to_string(),
      added_at: 1,
    });

    let decoded = VaultPayload::from_bytes(&payload.to_bytes().unwrap()).unwrap();

    assert_eq!(decoded.sync, payload.sync);
  }

  #[test]
  fn a_payload_from_before_sync_reads_as_never_synced() {
    let old = legacy::Payload {
      vault_id: Uuid::now_v7(),
      entries: vec![sample_entry().into()],
    };

    let decoded = VaultPayload::from_bytes(&legacy_bytes(&old)).unwrap();

    assert_eq!(decoded.sync, crate::SyncState::default());
  }

  fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
      .windows(needle.len())
      .any(|window| window == needle)
  }

  #[test]
  fn different_vault_ids_are_not_equal() {
    // Uuid::now_v7 is time-ordered, not random-only, so this is a
    // basic sanity check rather than a strong uniqueness proof —
    // real uniqueness comes from the timestamp + random tail, not
    // tested here since it's uuid's own concern, not vault-core's.
    let a = VaultPayload::new();
    let b = VaultPayload::new();
    assert_ne!(a.vault_id, b.vault_id);
  }
}
