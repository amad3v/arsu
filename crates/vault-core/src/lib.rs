//! The vault file format: the on-disk envelope and the structures it
//! encrypts.
//!
//! This crate is pure data and (de)serialization — no filesystem I/O
//! (that's `storage`'s job) and no encryption (that's `crypto`'s job).
//! Its only responsibility is "what does a vault look like on disk and
//! once decrypted," which is deliberately the narrowest possible scope
//! for the crate that defines the shape everything else has to agree on.

mod account;
pub mod cbor;
mod envelope;
mod payload;
mod secret;
mod sync_state;

pub use account::AccountKey;
pub use envelope::{
  CURRENT_FORMAT_VERSION, EnvelopeHeader, HEADER_LEN, LEGACY_FORMAT_VERSION, MAGIC, VaultEnvelope,
};
pub use payload::{Entry, OtpConfig, Tombstone, VaultPayload};
pub use secret::SecretBytes;
pub use sync_state::{
  DeviceIdentity, DeviceSet, Member, PUBLIC_KEY_LEN, PendingInvitation, PublicKey, Removal,
  SyncState,
};

use crypto::KdfParamsError;

#[derive(Debug, thiserror::Error)]
pub enum VaultCoreError {
  #[error("not a recognized vault file (missing or wrong magic bytes)")]
  BadMagic,
  #[error("file is too short to be a valid vault envelope")]
  Truncated,
  #[error(
    "unsupported vault format version {found} (this build reads versions \
     {LEGACY_FORMAT_VERSION} to {CURRENT_FORMAT_VERSION})"
  )]
  UnsupportedVersion { found: u16 },
  /// Checked while parsing the header, *before* anything derives a key
  /// with the parameters — see [`crypto::KdfParams`].
  #[error("vault header has unsupported key-derivation parameters")]
  UnsupportedKdfParams(#[from] KdfParamsError),
  #[error("failed to encode vault data")]
  Encode(#[source] ciborium::ser::Error<std::io::Error>),
  #[error("failed to decode vault data")]
  Decode(#[source] ciborium::de::Error<std::io::Error>),
}
