//! Import from other authenticator apps' backup files (Aegis's JSON
//! vault and 2FAS's `.2fas` backup), and export to Aegis.
//!
//! Both formats come from outside this app, so everything here treats
//! its input as untrusted, possibly hostile data:
//! - Files larger than [`MAX_IMPORT_BYTES`] are refused.
//! - Key-derivation parameters read from a file are checked against
//!   explicit bounds before any key is derived, so a crafted file cannot
//!   make the app allocate or compute without limit.
//! - JSON is parsed with `serde_json`, whose recursion limit (and
//!   iterative skipping of unknown fields) keeps deep nesting from
//!   overflowing the stack.
//! - Each entry is parsed and validated on its own: a malformed or
//!   unsupported entry is reported in [`ImportOutcome::skipped`] and the
//!   rest still import.
//!
//! Imported accounts are `qr::ParsedAccount`s, built through the same
//! decoding and validation as a scanned QR code or a manual entry: bulk
//! import has no separate, looser path.

pub mod aegis;
mod outcome;
mod secret;
pub mod twofas;

pub use outcome::{ImportOutcome, KnownAccounts, SkipReason, SkippedEntry};

/// The largest backup file the importers accept, 64 MiB. Real backups
/// are far smaller, but Aegis embeds custom icons, which can add up.
///
/// Callers should enforce the limit while reading, for example with
/// `Read::take(MAX_IMPORT_BYTES + 1)`, so that an oversized or endless
/// file is never read into memory. The importers check it again.
pub const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum InteropError {
  #[error("the file is larger than the {} MiB import limit", MAX_IMPORT_BYTES >> 20)]
  FileTooLarge,
  #[error("not a recognized file for this format")]
  UnrecognizedFormat(#[source] serde_json::Error),
  #[error("this file's format version ({found}) is not supported")]
  UnsupportedVersion { found: u32 },
  #[error("a password is required to decrypt this file")]
  PasswordRequired,
  #[error("incorrect password, or the file is corrupted")]
  WrongPasswordOrCorrupted,
  #[error(
    "this file can only be unlocked with a credential this app doesn't support (e.g. biometrics)"
  )]
  UnsupportedCredential,
  #[error("the file has more key slots than any real backup")]
  TooManySlots,
  /// Checked before deriving any key: a crafted file could otherwise
  /// ask for unbounded memory or time.
  #[error(
    "the file's key-derivation parameters are outside the supported range (scrypt N={n}, r={r}, p={p})"
  )]
  UnsupportedKdfParams { n: u32, r: u32, p: u32 },
  #[error("the file's {0} is missing or malformed")]
  Malformed(&'static str),
  #[error("the system random number generator failed")]
  Randomness(#[source] getrandom::Error),
  #[error("failed to encrypt the export")]
  Encrypt(#[source] aes_gcm::Error),
  #[error("failed to serialize the export")]
  Serialize(#[source] serde_json::Error),
}

/// Refuses input over [`MAX_IMPORT_BYTES`].
fn check_size(file_bytes: &[u8]) -> Result<(), InteropError> {
  if u64::try_from(file_bytes.len()).is_ok_and(|len| len <= MAX_IMPORT_BYTES) {
    Ok(())
  } else {
    Err(InteropError::FileTooLarge)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn importers_refuse_oversized_input() {
    let oversized = vec![b' '; usize::try_from(MAX_IMPORT_BYTES).unwrap() + 1];
    let known = KnownAccounts::default();

    assert!(matches!(
      aegis::import(&oversized, None, &known),
      Err(InteropError::FileTooLarge)
    ));
    assert!(matches!(
      twofas::import(&oversized, None, &known),
      Err(InteropError::FileTooLarge)
    ));
    assert!(check_size(&oversized[1..]).is_ok());
  }
}
