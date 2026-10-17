//! CBOR for plaintext that holds secrets.
//!
//! The vault payload, a sync snapshot and the fields a merge compares
//! all carry seeds, so they share one careful encoder and decoder:
//!
//! * [`to_zeroizing_vec`] sizes its buffer exactly before writing (by
//!   encoding once into a byte counter), because a growing `Vec` leaves
//!   a partial plaintext copy in every allocation it outgrows; the one
//!   allocation that remains is zeroized on drop.
//! * [`from_slice`] stages every string and byte string in a scratch
//!   buffer that is zeroized afterwards.

use std::io;

use serde::{Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

use crate::VaultCoreError;

/// Size of the buffer decoding stages strings and byte strings in
/// (ciborium's own default).
const DECODE_SCRATCH_LEN: usize = 4096;

/// `value` as CBOR, in a buffer allocated once and zeroized on drop.
///
/// # Errors
///
/// [`VaultCoreError::Encode`] if `value` cannot be encoded as CBOR.
pub fn to_zeroizing_vec<T: Serialize + ?Sized>(
  value: &T,
) -> Result<Zeroizing<Vec<u8>>, VaultCoreError> {
  let mut counter = ByteCounter::default();
  ciborium::into_writer(value, &mut counter).map_err(VaultCoreError::Encode)?;

  let mut out = Zeroizing::new(Vec::with_capacity(counter.0));
  ciborium::into_writer(value, &mut *out).map_err(VaultCoreError::Encode)?;
  Ok(out)
}

/// Decodes CBOR, staging strings in a buffer zeroized afterwards.
///
/// # Errors
///
/// [`VaultCoreError::Decode`] if `bytes` is not valid CBOR for `T`.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, VaultCoreError> {
  let mut scratch = Zeroizing::new([0u8; DECODE_SCRATCH_LEN]);
  ciborium::de::from_reader_with_buffer(bytes, scratch.as_mut_slice())
    .map_err(VaultCoreError::Decode)
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

#[cfg(test)]
mod tests {
  use serde::Deserialize;

  use super::*;

  #[derive(Debug, PartialEq, Serialize, Deserialize)]
  struct Sample {
    name: String,
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
  }

  fn sample() -> Sample {
    Sample {
      name: "x".repeat(300),
      data: vec![7; 5000],
    }
  }

  #[test]
  fn round_trips() {
    let bytes = to_zeroizing_vec(&sample()).unwrap();

    assert_eq!(from_slice::<Sample>(&bytes).unwrap(), sample());
  }

  #[test]
  fn the_buffer_is_allocated_exactly_once() {
    let bytes = to_zeroizing_vec(&sample()).unwrap();

    assert_eq!(bytes.capacity(), bytes.len());
  }

  #[test]
  fn invalid_input_is_a_decode_error() {
    assert!(matches!(
      from_slice::<Sample>(b"\xff\x00"),
      Err(VaultCoreError::Decode(_))
    ));
  }
}
