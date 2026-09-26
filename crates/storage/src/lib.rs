//! Everything that touches the filesystem: the encrypted vault file
//! (locking, crash-safe writes, backups) and the settings file.
//!
//! `vault-core` defines what the vault bytes mean and `crypto` encrypts
//! them; this crate only gets bytes to and from disk safely.

mod error;
mod fs_util;
mod paths;
mod settings;
mod vault;

pub use error::StorageError;
pub use fs_util::atomic_write;
pub use paths::AppPaths;
pub use settings::{
  AutoLockMinutes, ClipboardClearSeconds, InvalidSetting, Settings, SettingsStore, Theme,
};
pub use vault::{MAX_BACKUPS, UnlockedVault, VaultStorage};
