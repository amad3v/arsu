//! Secret material both backup formats share: AES-256-GCM, the keys
//! it runs under, and seeds stored as text.

use std::{fmt, io};

use aes_gcm::{AeadInOut, Aes256Gcm, KeyInit};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::InteropError;

pub(crate) const KEY_LEN: usize = 32;
pub(crate) const NONCE_LEN: usize = 12;
pub(crate) const TAG_LEN: usize = 16;

/// An AES-256 key, zeroized on drop.
pub(crate) type Key = Zeroizing<[u8; KEY_LEN]>;

/// Decrypts `buffer` in place with AES-256-GCM and no associated data,
/// checking it against the detached `tag`.
///
/// # Errors
///
/// Fails if the ciphertext does not authenticate under `key`: the key is
/// wrong or the data was modified.
pub(crate) fn open(
  key: &[u8; KEY_LEN],
  nonce: &[u8; NONCE_LEN],
  buffer: &mut [u8],
  tag: &[u8; TAG_LEN],
) -> Result<(), aes_gcm::Error> {
  Aes256Gcm::new(key.into()).decrypt_inout_detached(nonce.into(), &[], buffer.into(), tag.into())
}

/// Encrypts `buffer` in place with AES-256-GCM and no associated data,
/// returning the detached tag.
///
/// # Errors
///
/// Returns [`InteropError::Encrypt`] if `buffer` is longer than
/// AES-GCM can encrypt under one nonce (64 GiB).
pub(crate) fn seal(
  key: &[u8; KEY_LEN],
  nonce: &[u8; NONCE_LEN],
  buffer: &mut [u8],
) -> Result<[u8; TAG_LEN], InteropError> {
  Aes256Gcm::new(key.into())
    .encrypt_inout_detached(nonce.into(), &[], buffer.into())
    .map(Into::into)
    .map_err(InteropError::Encrypt)
}

/// `N` bytes from the operating system's random number generator, in a
/// buffer zeroized on drop (they may be a key).
///
/// # Errors
///
/// Returns [`InteropError::Randomness`] if the generator fails.
pub(crate) fn random_bytes<const N: usize>() -> Result<Zeroizing<[u8; N]>, InteropError> {
  let mut bytes = Zeroizing::new([0; N]);
  getrandom::fill(bytes.as_mut_slice()).map_err(InteropError::Randomness)?;
  Ok(bytes)
}

/// Serializes `value` as JSON into a buffer that is zeroized on drop.
///
/// The buffer is sized exactly before anything is written (by
/// serializing once into a byte counter), because a growing `Vec`
/// leaves a partial copy of the plaintext behind in every allocation it
/// outgrows.
///
/// # Errors
///
/// Returns [`InteropError::Serialize`] if `value` cannot be serialized.
pub(crate) fn to_json_zeroizing<T: Serialize>(
  value: &T,
) -> Result<Zeroizing<Vec<u8>>, InteropError> {
  let mut counter = ByteCounter::default();
  serde_json::to_writer(&mut counter, value).map_err(InteropError::Serialize)?;

  let mut json = Zeroizing::new(Vec::with_capacity(counter.0));
  serde_json::to_writer(&mut *json, value).map_err(InteropError::Serialize)?;
  Ok(json)
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

/// A secret in the text form a backup file stores it in: a base32 seed,
/// or an `otpauth://` URI that carries one. Zeroized on drop, and its
/// `Debug` output is redacted.
pub(crate) struct SecretText(Zeroizing<String>);

impl SecretText {
  pub(crate) fn expose_secret(&self) -> &str {
    &self.0
  }

  /// Blank text is no secret: backups write `""` for a missing field.
  pub(crate) fn is_blank(&self) -> bool {
    self.0.trim().is_empty()
  }
}

impl From<Zeroizing<String>> for SecretText {
  fn from(text: Zeroizing<String>) -> Self {
    Self(text)
  }
}

impl fmt::Debug for SecretText {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("SecretText([REDACTED])")
  }
}

impl Serialize for SecretText {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(self.expose_secret())
  }
}

impl<'de> Deserialize<'de> for SecretText {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    String::deserialize(deserializer).map(|text| Self(Zeroizing::new(text)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn seal_then_open_round_trips_and_detects_tampering() {
    let key = [7; KEY_LEN];
    let nonce = [9; NONCE_LEN];
    let mut buffer = b"attack at dawn".to_vec();

    let tag = seal(&key, &nonce, &mut buffer).unwrap();
    assert_ne!(buffer, b"attack at dawn");

    let mut tampered = buffer.clone();
    tampered[0] ^= 1;
    assert!(open(&key, &nonce, &mut tampered, &tag).is_err());
    assert!(open(&[8; KEY_LEN], &nonce, &mut buffer.clone(), &tag).is_err());

    open(&key, &nonce, &mut buffer, &tag).unwrap();
    assert_eq!(buffer, b"attack at dawn");
  }

  #[test]
  fn json_buffer_is_sized_exactly() {
    let value = vec!["JBSWY3DPEHPK3PXP"; 100];
    let json = to_json_zeroizing(&value).unwrap();
    assert_eq!(json.capacity(), json.len());
    assert_eq!(*json, serde_json::to_vec(&value).unwrap());
  }

  #[test]
  fn secret_text_debug_is_redacted() {
    let secret: SecretText = serde_json::from_str(r#""JBSWY3DPEHPK3PXP""#).unwrap();
    assert_eq!(format!("{secret:?}"), "SecretText([REDACTED])");
    assert_eq!(secret.expose_secret(), "JBSWY3DPEHPK3PXP");
    assert_eq!(
      serde_json::to_string(&secret).unwrap(),
      r#""JBSWY3DPEHPK3PXP""#
    );
  }
}
