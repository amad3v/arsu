use std::{
  io,
  path::{Path, PathBuf},
};

use crypto::CryptoError;
use vault_core::VaultCoreError;

/// Everything that can go wrong reading or writing the app's files.
///
/// Wrapped causes are exposed through [`std::error::Error::source`]
/// rather than repeated in the `Display` text, so an error chain prints
/// each cause once and callers can still inspect, say, the
/// [`io::ErrorKind`] of a failed write (a full disk vs. a permission
/// problem).
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
  #[error("a vault already exists at {0}")]
  AlreadyExists(PathBuf),
  #[error("no vault found at {0}")]
  NotFound(PathBuf),
  /// Another instance of the app (or another [`crate::VaultStorage`]
  /// in this process) holds the vault's lock.
  #[error("the vault at {0} is already open in another instance of the app")]
  InUse(PathBuf),
  #[error("could not determine the data and config directories for this platform")]
  NoProjectDirs,
  #[error("file system operation failed on {path}")]
  Io {
    path: PathBuf,
    #[source]
    source: io::Error,
  },
  #[error(transparent)]
  Crypto(#[from] CryptoError),
  #[error(transparent)]
  VaultCore(#[from] VaultCoreError),
  #[error("the settings JSON for {path} is invalid")]
  Settings {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
}

impl StorageError {
  /// `map_err` adapter attaching the path an I/O operation was about.
  pub(crate) fn io(path: &Path) -> impl FnOnce(io::Error) -> Self + '_ {
    move |source| Self::Io {
      path: path.to_path_buf(),
      source,
    }
  }
}
