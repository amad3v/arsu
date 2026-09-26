//! [`SecretBytes`]: the one type OTP seeds live in.

use std::{
  fmt,
  hash::{Hash, Hasher},
};

use ctutils::CtEq;
use serde::{
  Deserialize, Deserializer, Serialize, Serializer,
  de::{self, SeqAccess, Visitor},
};
use zeroize::Zeroizing;

/// Secret key material — an OTP seed.
///
/// - **Zeroized on drop**, so locking the vault (which drops the
///   decrypted payload) scrubs every seed instead of leaving it in freed
///   heap memory. Clones are separate zeroizing buffers.
/// - **Redacted `Debug`**, so the types that hold it (`OtpConfig`,
///   `Entry`, `VaultPayload`, …) can derive `Debug` without a stray
///   `dbg!` or log line printing a seed.
/// - **Constant-time `PartialEq`**, so comparing two seeds does not leak
///   how many leading bytes match.
/// - **Serializes as a byte string**, byte-for-byte the representation
///   the vault payload has always used for seeds.
///
/// Reading the bytes is always an explicit [`SecretBytes::expose_secret`]
/// call, which keeps every place that touches a raw seed greppable.
#[derive(Clone)]
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
  #[must_use]
  pub fn expose_secret(&self) -> &[u8] {
    &self.0
  }

  #[must_use]
  pub fn len(&self) -> usize {
    self.0.len()
  }

  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }
}

impl From<Vec<u8>> for SecretBytes {
  fn from(bytes: Vec<u8>) -> Self {
    Self(Zeroizing::new(bytes))
  }
}

impl From<Zeroizing<Vec<u8>>> for SecretBytes {
  fn from(bytes: Zeroizing<Vec<u8>>) -> Self {
    Self(bytes)
  }
}

impl fmt::Debug for SecretBytes {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("SecretBytes([REDACTED])")
  }
}

impl PartialEq for SecretBytes {
  fn eq(&self, other: &Self) -> bool {
    self.expose_secret().ct_eq(other.expose_secret()).to_bool()
  }
}

impl Eq for SecretBytes {}

/// Consistent with `PartialEq`: equal secrets hash equally. Only for
/// in-memory sets (e.g. import de-duplication); the hash never leaves
/// the process.
impl Hash for SecretBytes {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.expose_secret().hash(state);
  }
}

impl Serialize for SecretBytes {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(self.expose_secret())
  }
}

impl<'de> Deserialize<'de> for SecretBytes {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    deserializer.deserialize_bytes(SecretBytesVisitor)
  }
}

struct SecretBytesVisitor;

impl<'de> Visitor<'de> for SecretBytesVisitor {
  type Value = SecretBytes;

  fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("a byte string")
  }

  /// The path CBOR takes: the bytes are copied once, into an exactly
  /// sized zeroizing buffer.
  fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<SecretBytes, E> {
    Ok(bytes.to_vec().into())
  }

  fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<SecretBytes, E> {
    Ok(bytes.into())
  }

  /// For formats without a native byte string (e.g. JSON arrays). The
  /// length hint comes from the input, so it only sizes the initial
  /// allocation up to a bound no real seed exceeds.
  fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<SecretBytes, A::Error> {
    const MAX_PREALLOCATED: usize = 256;
    let capacity = seq.size_hint().unwrap_or(0).min(MAX_PREALLOCATED);
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    while let Some(byte) = seq.next_element()? {
      bytes.push(byte);
    }
    Ok(bytes.into())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn debug_output_is_redacted() {
    let secret = SecretBytes::from(b"JBSWY3DPEHPK3PXP".to_vec());
    let printed = format!("{secret:?}");
    assert_eq!(printed, "SecretBytes([REDACTED])");
    assert!(!printed.contains("74")); // 'J' as a decimal byte
  }

  #[test]
  fn equality_compares_content_and_length() {
    let a = SecretBytes::from(b"12345678901234567890".to_vec());
    assert_eq!(a, SecretBytes::from(b"12345678901234567890".to_vec()));
    assert_ne!(a, SecretBytes::from(b"12345678901234567891".to_vec()));
    assert_ne!(a, SecretBytes::from(b"1234567890123456789".to_vec()));
  }

  #[test]
  fn serializes_exactly_like_a_plain_byte_string() {
    // The vault payload has always stored seeds as
    // `#[serde(with = "serde_bytes")] Vec<u8>`; existing vaults must
    // decode into `SecretBytes` unchanged.
    let raw = b"12345678901234567890".to_vec();

    let mut legacy = Vec::new();
    ciborium::into_writer(&serde_bytes::Bytes::new(&raw), &mut legacy).unwrap();
    let mut current = Vec::new();
    ciborium::into_writer(&SecretBytes::from(raw.clone()), &mut current).unwrap();
    assert_eq!(current, legacy);

    let decoded: SecretBytes = ciborium::from_reader(legacy.as_slice()).unwrap();
    assert_eq!(decoded.expose_secret(), raw.as_slice());
  }
}
