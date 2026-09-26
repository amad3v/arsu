//! The decrypted contents of a vault.

use std::io;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use otp::Algorithm;

use crate::{SecretBytes, VaultCoreError};

/// Size of the buffer CBOR decoding stages strings and byte strings in
/// (ciborium's own default).
const DECODE_SCRATCH_LEN: usize = 4096;

/// The decrypted contents of a vault: everything inside the ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultPayload {
  /// Stable identity for this vault. Not used for anything in v1, but
  /// it's what a future sync protocol needs to tell "two copies of the
  /// same vault" apart from "two different vaults that happen to have
  /// similar entries".
  pub vault_id: Uuid,
  pub entries: Vec<Entry>,
}

impl VaultPayload {
  #[must_use]
  pub fn new() -> Self {
    Self {
      vault_id: Uuid::now_v7(),
      entries: Vec::new(),
    }
  }

  /// Serialize to CBOR — the plaintext that gets encrypted.
  ///
  /// The buffer is sized exactly before anything is written (by
  /// serializing once into a byte counter), because a growing `Vec`
  /// leaves a partial plaintext copy in every allocation it outgrows;
  /// the one allocation that remains is zeroized on drop.
  ///
  /// # Errors
  ///
  /// Returns [`VaultCoreError::Encode`] if the payload cannot be
  /// serialized as CBOR.
  pub fn to_bytes(&self) -> Result<Zeroizing<Vec<u8>>, VaultCoreError> {
    let mut counter = ByteCounter::default();
    ciborium::into_writer(self, &mut counter).map_err(VaultCoreError::Encode)?;

    let mut out = Zeroizing::new(Vec::with_capacity(counter.0));
    ciborium::into_writer(self, &mut *out).map_err(VaultCoreError::Encode)?;
    Ok(out)
  }

  /// Parse decrypted CBOR. The decoder stages every string and byte
  /// string — seeds included — in a scratch buffer, which is zeroized
  /// afterwards.
  ///
  /// # Errors
  ///
  /// Returns [`VaultCoreError::Decode`] if the bytes cannot be
  /// deserialized as a vault payload.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, VaultCoreError> {
    let mut scratch = Zeroizing::new([0u8; DECODE_SCRATCH_LEN]);
    ciborium::de::from_reader_with_buffer(bytes, scratch.as_mut_slice())
      .map_err(VaultCoreError::Decode)
  }

  /// Entries not marked deleted — what the UI actually lists.
  pub fn active_entries(&self) -> impl Iterator<Item = &Entry> {
    self.entries.iter().filter(|entry| entry.is_active())
  }
}

impl Default for VaultPayload {
  fn default() -> Self {
    Self::new()
  }
}

/// Counts the bytes written to it and discards them.
#[derive(Default)]
struct ByteCounter(usize);

impl io::Write for ByteCounter {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.0 += buf.len();
    Ok(buf.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
  /// Tombstone, not hard-delete — deliberate: this
  /// is what a future sync merge needs to tell "never existed on this
  /// device" apart from "deleted on another device."
  pub deleted_at: Option<u64>,
}

impl Entry {
  /// Is this entry live (not tombstoned)? The single definition every
  /// listing and lookup goes through.
  #[must_use]
  pub fn is_active(&self) -> bool {
    self.deleted_at.is_none()
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
      deleted_at: None,
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
  fn tombstoned_entry_is_excluded_from_active_entries() {
    let mut payload = VaultPayload::new();
    payload.entries.push(sample_entry());
    payload.entries.push(Entry {
      deleted_at: Some(1_700_000_500),
      ..sample_entry()
    });

    assert_eq!(payload.entries.len(), 2, "tombstones stay in the list");
    assert_eq!(
      payload.active_entries().count(),
      1,
      "but are excluded from active entries"
    );
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
