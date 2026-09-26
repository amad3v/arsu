//! The on-disk envelope: a plaintext header followed by the ciphertext.
//!
//! # Format version 2 (written by this build)
//!
//! A fixed-size binary header, all integers big-endian, followed by the
//! `XChaCha20-Poly1305` ciphertext (which ends in its 16-byte tag):
//!
//! | offset | size | field                                  |
//! | -----: | ---: | -------------------------------------- |
//! |      0 |    4 | [`MAGIC`] (`"VLT1"`)                   |
//! |      4 |    2 | format version (`2`)                   |
//! |      6 |    4 | Argon2id memory cost, KiB              |
//! |     10 |    4 | Argon2id passes                        |
//! |     14 |    4 | Argon2id lanes                         |
//! |     18 |   16 | salt                                   |
//! |     34 |   24 | nonce                                  |
//! |     58 |    … | ciphertext                             |
//!
//! The whole header — the first [`HEADER_LEN`] bytes — is the AEAD
//! associated data, so flipping any header byte (the version included)
//! makes decryption fail instead of, say, getting a ciphertext
//! interpreted under a different format version. The version sits right
//! after the magic, so a reader dispatches on it before parsing
//! anything else.
//!
//! The format version also names the algorithm suite: version 2 means
//! Argon2id v1.3 for the key and `XChaCha20-Poly1305` for the payload.
//! Changing either is a new format version.
//!
//! # Format version 1 (read only)
//!
//! The first releases wrote [`MAGIC`] followed by one CBOR map
//! (`format_version`, `kdf_params`, `salt`, `nonce`, `ciphertext`) and
//! authenticated no header. It is recognised by the byte after the
//! magic being a CBOR map header (major type 5, `0xA0..=0xBF`) — never
//! the first byte of a version-2 header, whose version field would have
//! to be 40960 or higher. Version-1 vaults still open; the next save
//! rewrites them as version 2.

use serde::Deserialize;

use crypto::{KdfParams, NONCE_LEN, SALT_LEN, Salt};

use crate::VaultCoreError;

/// 4-byte file signature, checked before anything else is parsed.
pub const MAGIC: [u8; 4] = *b"VLT1";

/// The format version this build writes.
pub const CURRENT_FORMAT_VERSION: u16 = 2;

/// The oldest format version this build still reads.
pub const LEGACY_FORMAT_VERSION: u16 = 1;

const VERSION_LEN: usize = size_of::<u16>();
const KDF_PARAMS_LEN: usize = 3 * size_of::<u32>();

/// Length of the version-2 header, which is also its associated data.
pub const HEADER_LEN: usize = MAGIC.len() + VERSION_LEN + KDF_PARAMS_LEN + SALT_LEN + NONCE_LEN;

/// CBOR major type 5 (map), in the top three bits of a header byte.
const CBOR_MAP_MAJOR_TYPE: u8 = 5;

/// Everything needed to attempt decryption. Plaintext by necessity —
/// the app has to know the KDF parameters and salt *before* it can
/// derive a key to decrypt anything — but authenticated as associated
/// data from format version 2 on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeHeader {
  pub kdf_params: KdfParams,
  pub salt: Salt,
  pub nonce: [u8; NONCE_LEN],
}

impl EnvelopeHeader {
  /// The header in the current format: the first [`HEADER_LEN`] bytes
  /// of the file, and the associated data its ciphertext must be
  /// encrypted with.
  #[must_use]
  pub fn to_bytes(&self) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&CURRENT_FORMAT_VERSION.to_be_bytes());
    for cost in [
      self.kdf_params.m_cost_kib(),
      self.kdf_params.t_cost(),
      self.kdf_params.p_cost(),
    ] {
      out.extend_from_slice(&cost.to_be_bytes());
    }
    out.extend_from_slice(&self.salt.0);
    out.extend_from_slice(&self.nonce);
    out
  }

  /// The complete vault file: this header followed by `ciphertext`,
  /// which must have been encrypted with [`Self::to_bytes`] as its
  /// associated data. `storage` writes this verbatim.
  #[must_use]
  pub fn encode(&self, ciphertext: &[u8]) -> Vec<u8> {
    let mut out = self.to_bytes();
    out.extend_from_slice(ciphertext);
    out
  }
}

/// A vault file parsed from disk, in any supported format version.
///
/// Read-only on purpose: the only way to produce vault bytes is
/// [`EnvelopeHeader::encode`], which always writes the current format.
#[derive(Debug)]
pub struct VaultEnvelope {
  format_version: u16,
  header: EnvelopeHeader,
  ciphertext: Vec<u8>,
}

impl VaultEnvelope {
  /// Parse and validate a vault file. Rejects anything that is not a
  /// recognised, supported vault — including KDF parameters outside
  /// [`KdfParams`]' bounds, so a crafted header cannot make the caller
  /// derive a key for minutes or forever.
  ///
  /// # Errors
  ///
  /// Returns [`VaultCoreError::Truncated`] if the input ends before the
  /// header does, [`VaultCoreError::BadMagic`] if it does not start with
  /// [`MAGIC`], [`VaultCoreError::UnsupportedVersion`] for a format
  /// version this build cannot read,
  /// [`VaultCoreError::UnsupportedKdfParams`] for out-of-bounds KDF
  /// parameters, or [`VaultCoreError::Decode`] if a version-1 header is
  /// not valid CBOR.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, VaultCoreError> {
    let (magic, body) = bytes
      .split_first_chunk::<{ MAGIC.len() }>()
      .ok_or(VaultCoreError::Truncated)?;
    if *magic != MAGIC {
      return Err(VaultCoreError::BadMagic);
    }

    match body.first() {
      None => Err(VaultCoreError::Truncated),
      Some(byte) if byte >> 5 == CBOR_MAP_MAJOR_TYPE => Self::from_v1(body),
      Some(_) => Self::from_v2(body),
    }
  }

  fn from_v2(mut fields: &[u8]) -> Result<Self, VaultCoreError> {
    let format_version = u16::from_be_bytes(take(&mut fields)?);
    if format_version != CURRENT_FORMAT_VERSION {
      return Err(VaultCoreError::UnsupportedVersion {
        found: format_version,
      });
    }

    let m_cost_kib = u32::from_be_bytes(take(&mut fields)?);
    let t_cost = u32::from_be_bytes(take(&mut fields)?);
    let p_cost = u32::from_be_bytes(take(&mut fields)?);
    let header = EnvelopeHeader {
      kdf_params: KdfParams::new(m_cost_kib, t_cost, p_cost)?,
      salt: Salt(take(&mut fields)?),
      nonce: take(&mut fields)?,
    };

    Ok(Self {
      format_version,
      header,
      ciphertext: fields.to_vec(),
    })
  }

  fn from_v1(cbor: &[u8]) -> Result<Self, VaultCoreError> {
    let legacy: EnvelopeV1 = ciborium::from_reader(cbor).map_err(VaultCoreError::Decode)?;
    if legacy.format_version != LEGACY_FORMAT_VERSION {
      return Err(VaultCoreError::UnsupportedVersion {
        found: legacy.format_version,
      });
    }

    let KdfParamsV1 {
      m_cost_kib,
      t_cost,
      p_cost,
    } = legacy.kdf_params;
    Ok(Self {
      format_version: LEGACY_FORMAT_VERSION,
      header: EnvelopeHeader {
        kdf_params: KdfParams::new(m_cost_kib, t_cost, p_cost)?,
        salt: Salt(legacy.salt),
        nonce: legacy.nonce,
      },
      ciphertext: legacy.ciphertext,
    })
  }

  #[must_use]
  pub const fn format_version(&self) -> u16 {
    self.format_version
  }

  #[must_use]
  pub const fn header(&self) -> &EnvelopeHeader {
    &self.header
  }

  #[must_use]
  pub fn ciphertext(&self) -> &[u8] {
    &self.ciphertext
  }

  /// The associated data the ciphertext was encrypted with: the
  /// serialized header for version 2 (whose fixed layout re-encodes to
  /// exactly the bytes that were read), nothing for version 1.
  #[must_use]
  pub fn associated_data(&self) -> Vec<u8> {
    if self.format_version == LEGACY_FORMAT_VERSION {
      Vec::new()
    } else {
      self.header.to_bytes()
    }
  }
}

/// Split the next `N` bytes off `input`.
fn take<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], VaultCoreError> {
  let (field, rest) = input
    .split_first_chunk::<N>()
    .ok_or(VaultCoreError::Truncated)?;
  *input = rest;
  Ok(*field)
}

/// The version-1 envelope exactly as the first releases wrote it.
/// Frozen: its field names and types *are* the legacy format.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct EnvelopeV1 {
  format_version: u16,
  kdf_params: KdfParamsV1,
  salt: [u8; SALT_LEN],
  nonce: [u8; NONCE_LEN],
  #[serde(with = "serde_bytes")]
  ciphertext: Vec<u8>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct KdfParamsV1 {
  m_cost_kib: u32,
  t_cost: u32,
  p_cost: u32,
}

#[cfg(test)]
mod tests {
  use ciborium::value::Value;

  use super::*;

  fn sample_header() -> EnvelopeHeader {
    EnvelopeHeader {
      kdf_params: KdfParams::MINIMUM,
      salt: Salt([7u8; SALT_LEN]),
      nonce: [9u8; NONCE_LEN],
    }
  }

  /// A version-1 file built from a hand-written CBOR map, independent
  /// of the private `EnvelopeV1` type the parser uses.
  fn v1_file(format_version: u16, t_cost: u32) -> Vec<u8> {
    let text = |s: &str| Value::Text(s.to_string());
    let array = |bytes: &[u8]| Value::Array(bytes.iter().map(|b| Value::from(*b)).collect());
    let envelope = Value::Map(vec![
      (text("format_version"), Value::from(format_version)),
      (
        text("kdf_params"),
        Value::Map(vec![
          (text("m_cost_kib"), Value::from(19 * 1024)),
          (text("t_cost"), Value::from(t_cost)),
          (text("p_cost"), Value::from(1)),
        ]),
      ),
      (text("salt"), array(&[7u8; SALT_LEN])),
      (text("nonce"), array(&[9u8; NONCE_LEN])),
      (text("ciphertext"), Value::Bytes(b"v1 ciphertext".to_vec())),
    ]);
    let mut file = MAGIC.to_vec();
    ciborium::into_writer(&envelope, &mut file).unwrap();
    file
  }

  #[test]
  fn header_layout_is_the_documented_one() {
    let bytes = sample_header().to_bytes();
    assert_eq!(bytes.len(), HEADER_LEN);
    assert_eq!(HEADER_LEN, 58);
    assert_eq!(&bytes[..4], b"VLT1");
    assert_eq!(&bytes[4..6], &[0, 2], "big-endian version 2");
    assert_eq!(&bytes[6..10], &(19u32 * 1024).to_be_bytes());
    assert_eq!(&bytes[10..14], &2u32.to_be_bytes());
    assert_eq!(&bytes[14..18], &1u32.to_be_bytes());
    assert_eq!(&bytes[18..34], &[7u8; SALT_LEN]);
    assert_eq!(&bytes[34..58], &[9u8; NONCE_LEN]);
  }

  #[test]
  fn v2_round_trips_and_authenticates_exactly_the_bytes_on_disk() {
    let header = sample_header();
    let file = header.encode(b"pretend ciphertext");

    let envelope = VaultEnvelope::from_bytes(&file).unwrap();

    assert_eq!(envelope.format_version(), CURRENT_FORMAT_VERSION);
    assert_eq!(envelope.header(), &header);
    assert_eq!(envelope.ciphertext(), b"pretend ciphertext");
    assert_eq!(envelope.associated_data(), &file[..HEADER_LEN]);
  }

  #[test]
  fn an_empty_ciphertext_parses_and_is_left_for_decryption_to_reject() {
    let file = sample_header().encode(&[]);
    assert!(
      VaultEnvelope::from_bytes(&file)
        .unwrap()
        .ciphertext()
        .is_empty()
    );
  }

  #[test]
  fn rejects_wrong_magic() {
    let mut file = sample_header().encode(b"x");
    file[0] ^= 0xff;
    assert!(matches!(
      VaultEnvelope::from_bytes(&file),
      Err(VaultCoreError::BadMagic)
    ));
  }

  #[test]
  fn rejects_truncated_input() {
    let file = sample_header().encode(&[]);
    for len in [0, 2, MAGIC.len(), MAGIC.len() + 1, 20, HEADER_LEN - 1] {
      assert!(
        matches!(
          VaultEnvelope::from_bytes(&file[..len]),
          Err(VaultCoreError::Truncated)
        ),
        "length {len}"
      );
    }
  }

  #[test]
  fn rejects_future_format_version_before_parsing_the_rest() {
    let mut file = sample_header().encode(b"x");
    file[4..6].copy_from_slice(&3u16.to_be_bytes());
    // A future version may use a different header: even a file too
    // short for a version-2 header must report the version, not
    // truncation.
    file.truncate(MAGIC.len() + VERSION_LEN);

    assert!(matches!(
      VaultEnvelope::from_bytes(&file),
      Err(VaultCoreError::UnsupportedVersion { found: 3 })
    ));
  }

  #[test]
  fn rejects_out_of_bounds_kdf_params_in_a_v2_header() {
    let mut file = sample_header().encode(b"x");
    file[10..14].copy_from_slice(&u32::MAX.to_be_bytes()); // t_cost
    assert!(matches!(
      VaultEnvelope::from_bytes(&file),
      Err(VaultCoreError::UnsupportedKdfParams(_))
    ));
  }

  #[test]
  fn reads_a_v1_envelope_with_no_associated_data() {
    let envelope = VaultEnvelope::from_bytes(&v1_file(1, 2)).unwrap();

    assert_eq!(envelope.format_version(), LEGACY_FORMAT_VERSION);
    assert_eq!(envelope.header(), &sample_header());
    assert_eq!(envelope.ciphertext(), b"v1 ciphertext");
    assert!(envelope.associated_data().is_empty());
  }

  #[test]
  fn legacy_struct_matches_the_hand_built_v1_encoding() {
    let legacy = EnvelopeV1 {
      format_version: 1,
      kdf_params: KdfParamsV1 {
        m_cost_kib: 19 * 1024,
        t_cost: 2,
        p_cost: 1,
      },
      salt: [7u8; SALT_LEN],
      nonce: [9u8; NONCE_LEN],
      ciphertext: b"v1 ciphertext".to_vec(),
    };
    let mut file = MAGIC.to_vec();
    ciborium::into_writer(&legacy, &mut file).unwrap();
    assert_eq!(file, v1_file(1, 2));
  }

  #[test]
  fn rejects_a_v1_shaped_envelope_with_another_version() {
    assert!(matches!(
      VaultEnvelope::from_bytes(&v1_file(99, 2)),
      Err(VaultCoreError::UnsupportedVersion { found: 99 })
    ));
  }

  #[test]
  fn rejects_out_of_bounds_kdf_params_in_a_v1_header() {
    assert!(matches!(
      VaultEnvelope::from_bytes(&v1_file(1, u32::MAX)),
      Err(VaultCoreError::UnsupportedKdfParams(_))
    ));
  }

  #[test]
  fn rejects_malformed_v1_cbor() {
    let mut file = v1_file(1, 2);
    file.truncate(file.len() - 3);
    assert!(matches!(
      VaultEnvelope::from_bytes(&file),
      Err(VaultCoreError::Decode(_))
    ));
  }
}
