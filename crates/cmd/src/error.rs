//! The one error type every command returns, and how it crosses IPC.
//!
//! On the wire an [`AppError`] is an object,
//! `{ "kind": "<AppErrorKind>", "message": "<text>" }`:
//! - `kind` is for the frontend's control flow. Every variant of every
//!   error the commands can produce maps to exactly one [`AppErrorKind`]
//!   (the exhaustive matches below make a new variant a compile error
//!   until it has one), and the frontend branches on it — never on the
//!   message.
//! - `message` is for people: the error followed by its causes. None of
//!   the errors ever carries a secret, so it is safe to show as-is.

use std::{error::Error, io, iter, path::PathBuf, time::SystemTimeError};

use serde::{Serialize, Serializer, ser::SerializeStruct};

use crypto::CryptoError;
use interop::InteropError;
use otp::OtpError;
use qr::QrError;
use storage::{InvalidSetting, StorageError};
use vault_core::VaultCoreError;

use crate::{clipboard::ClipboardError, password::MIN_PASSWORD_CHARS};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
  /// The vault or settings file could not be read or written.
  #[error(transparent)]
  Storage(#[from] StorageError),
  /// Deriving a key to re-check the master password failed.
  #[error(transparent)]
  Crypto(#[from] CryptoError),
  /// An account to add is invalid (from a URI or the manual form).
  #[error(transparent)]
  Account(#[from] QrError),
  /// A code could not be generated.
  #[error(transparent)]
  Otp(#[from] OtpError),
  /// Importing another app's backup, or exporting to Aegis, failed.
  #[error(transparent)]
  Transfer(#[from] InteropError),
  #[error(transparent)]
  InvalidSetting(#[from] InvalidSetting),
  #[error("the vault is locked")]
  Locked,
  #[error("incorrect password")]
  WrongPassword,
  #[error("the password must be at least {min} characters long", min = MIN_PASSWORD_CHARS)]
  WeakPassword,
  #[error("the entry does not exist")]
  EntryNotFound,
  #[error("'{0}' is not a valid entry id")]
  InvalidEntryId(String),
  #[error("a code to copy must be 6 to 8 digits")]
  InvalidCode,
  #[error("could not use the clipboard")]
  Clipboard(#[source] ClipboardError),
  #[error("no file is waiting to be imported; choose the file again")]
  NoPendingImport,
  #[error("{0} is not a regular file")]
  NotAFile(PathBuf),
  #[error("could not read {path}")]
  FileRead {
    path: PathBuf,
    #[source]
    source: io::Error,
  },
  #[error("could not write {path}")]
  FileWrite {
    path: PathBuf,
    #[source]
    source: io::Error,
  },
  /// The file dialog returned a location that is not a local file.
  #[error("the chosen location is not a file on this computer")]
  InvalidFilePath,
  /// No application opened a link (no browser, or the portal refused).
  #[error("could not open the link")]
  OpenLink(#[source] gtk::glib::Error),
  /// The file dialog could not be shown.
  #[error("could not open the file dialog")]
  FileDialog(#[source] tauri::Error),
  /// A file picked for import is neither an Aegis (`.json`) nor a 2FAS
  /// (`.2fas`) backup, going by its extension.
  #[error("{0} is not an Aegis (.json) or 2FAS (.2fas) backup")]
  UnsupportedFileType(PathBuf),
  #[error("the system clock is set before 1970")]
  SystemClock(#[source] SystemTimeError),
  #[error("a background task failed")]
  BackgroundTask(#[source] tauri::Error),
}

/// What went wrong, as the frontend branches on it. Serialized as the
/// variant name. Each command's `# Errors` (in [`crate::commands`])
/// lists the kinds it produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum AppErrorKind {
  // The vault and its session.
  Locked,
  VaultNotFound,
  VaultAlreadyExists,
  VaultInUse,
  WrongPassword,
  WeakPassword,
  NotAVault,
  VaultTruncated,
  UnsupportedVaultVersion,
  UnsupportedKdfParams,
  VaultEncode,
  VaultDecode,
  KeyDerivation,
  Encryption,
  Randomness,
  StorageIo,
  NoProjectDirs,
  // Settings.
  InvalidSettingsFile,
  InvalidAutoLockMinutes,
  InvalidClipboardClearSeconds,
  // Entries.
  EntryNotFound,
  InvalidEntryId,
  // Accounts being added (otpauth:// URIs, the manual form, imports).
  NotOtpauth,
  MissingLabel,
  LabelContainsColon,
  InvalidLabelEncoding,
  MissingSecret,
  InvalidSecret,
  MissingCounter,
  InvalidNumber,
  UnknownOtpType,
  UnknownAlgorithm,
  InvalidDigits,
  InvalidPeriod,
  EmptySecret,
  CounterExhausted,
  QrEncode,
  // The clipboard.
  InvalidCode,
  Clipboard,
  // Import and export files.
  NoPendingImport,
  NotAFile,
  FileRead,
  FileWrite,
  InvalidFilePath,
  FileDialog,
  // The About dialog.
  OpenLink,
  UnsupportedFileType,
  FileTooLarge,
  UnrecognizedFormat,
  UnsupportedFileVersion,
  PasswordRequired,
  WrongPasswordOrCorrupted,
  UnsupportedCredential,
  TooManySlots,
  UnsupportedFileKdfParams,
  MalformedFile,
  ExportSerialize,
  // The runtime.
  SystemClock,
  BackgroundTask,
}

impl AppError {
  #[must_use]
  pub fn kind(&self) -> AppErrorKind {
    match self {
      Self::Storage(error) => storage_kind(error),
      Self::Crypto(error) => crypto_kind(error),
      Self::Account(error) => account_kind(error),
      Self::Otp(error) => otp_kind(error),
      Self::Transfer(error) => transfer_kind(error),
      Self::InvalidSetting(InvalidSetting::AutoLockMinutes(_)) => {
        AppErrorKind::InvalidAutoLockMinutes
      }
      Self::InvalidSetting(InvalidSetting::ClipboardClearSeconds(_)) => {
        AppErrorKind::InvalidClipboardClearSeconds
      }
      Self::Locked => AppErrorKind::Locked,
      Self::WrongPassword => AppErrorKind::WrongPassword,
      Self::WeakPassword => AppErrorKind::WeakPassword,
      Self::EntryNotFound => AppErrorKind::EntryNotFound,
      Self::InvalidEntryId(_) => AppErrorKind::InvalidEntryId,
      Self::InvalidCode => AppErrorKind::InvalidCode,
      Self::Clipboard(_) => AppErrorKind::Clipboard,
      Self::NoPendingImport => AppErrorKind::NoPendingImport,
      Self::NotAFile(_) => AppErrorKind::NotAFile,
      Self::FileRead { .. } => AppErrorKind::FileRead,
      Self::FileWrite { .. } => AppErrorKind::FileWrite,
      Self::InvalidFilePath => AppErrorKind::InvalidFilePath,
      Self::FileDialog(_) => AppErrorKind::FileDialog,
      Self::OpenLink(_) => AppErrorKind::OpenLink,
      Self::UnsupportedFileType(_) => AppErrorKind::UnsupportedFileType,
      Self::SystemClock(_) => AppErrorKind::SystemClock,
      Self::BackgroundTask(_) => AppErrorKind::BackgroundTask,
    }
  }

  /// The error followed by each of its causes (see [`describe`]).
  #[must_use]
  pub fn message(&self) -> String {
    describe(self)
  }
}

/// `error` followed by each of its causes, `": "`-separated — for
/// example `could not write /x/y: Permission denied (os error 13)`.
#[must_use]
pub fn describe(error: &dyn Error) -> String {
  iter::successors(Some(error), |&error| error.source())
    .map(ToString::to_string)
    .collect::<Vec<_>>()
    .join(": ")
}

impl Serialize for AppError {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let mut payload = serializer.serialize_struct("AppError", 2)?;
    payload.serialize_field("kind", &self.kind())?;
    payload.serialize_field("message", &self.message())?;
    payload.end()
  }
}

fn storage_kind(error: &StorageError) -> AppErrorKind {
  match error {
    StorageError::AlreadyExists(_) => AppErrorKind::VaultAlreadyExists,
    StorageError::NotFound(_) => AppErrorKind::VaultNotFound,
    StorageError::InUse(_) => AppErrorKind::VaultInUse,
    StorageError::NoProjectDirs => AppErrorKind::NoProjectDirs,
    StorageError::Io { .. } => AppErrorKind::StorageIo,
    StorageError::Crypto(error) => crypto_kind(error),
    StorageError::VaultCore(error) => vault_kind(error),
    StorageError::Settings { .. } => AppErrorKind::InvalidSettingsFile,
  }
}

fn vault_kind(error: &VaultCoreError) -> AppErrorKind {
  match error {
    VaultCoreError::BadMagic => AppErrorKind::NotAVault,
    VaultCoreError::Truncated => AppErrorKind::VaultTruncated,
    VaultCoreError::UnsupportedVersion { .. } => AppErrorKind::UnsupportedVaultVersion,
    VaultCoreError::UnsupportedKdfParams(_) => AppErrorKind::UnsupportedKdfParams,
    VaultCoreError::Encode(_) => AppErrorKind::VaultEncode,
    VaultCoreError::Decode(_) => AppErrorKind::VaultDecode,
  }
}

fn crypto_kind(error: &CryptoError) -> AppErrorKind {
  match error {
    CryptoError::Kdf(_) => AppErrorKind::KeyDerivation,
    CryptoError::Encrypt => AppErrorKind::Encryption,
    // The AEAD can't tell a wrong password from a tampered file, and the
    // app doesn't try to: both are one error.
    CryptoError::Decrypt => AppErrorKind::WrongPassword,
    CryptoError::Rng(_) => AppErrorKind::Randomness,
  }
}

fn account_kind(error: &QrError) -> AppErrorKind {
  match error {
    QrError::NotOtpauth => AppErrorKind::NotOtpauth,
    QrError::MissingLabel => AppErrorKind::MissingLabel,
    QrError::LabelContainsColon => AppErrorKind::LabelContainsColon,
    QrError::InvalidLabelEncoding => AppErrorKind::InvalidLabelEncoding,
    QrError::MissingSecret => AppErrorKind::MissingSecret,
    QrError::InvalidSecret => AppErrorKind::InvalidSecret,
    QrError::MissingCounter => AppErrorKind::MissingCounter,
    QrError::InvalidNumber(_) => AppErrorKind::InvalidNumber,
    QrError::UnknownType(_) => AppErrorKind::UnknownOtpType,
    QrError::UnknownAlgorithm(_) => AppErrorKind::UnknownAlgorithm,
    QrError::InvalidOtpConfig(error) => otp_kind(error),
    QrError::QrEncode(_) => AppErrorKind::QrEncode,
  }
}

fn otp_kind(error: &OtpError) -> AppErrorKind {
  match error {
    OtpError::InvalidDigits(_) => AppErrorKind::InvalidDigits,
    OtpError::InvalidPeriod { .. } => AppErrorKind::InvalidPeriod,
    OtpError::EmptySecret => AppErrorKind::EmptySecret,
    OtpError::CounterExhausted => AppErrorKind::CounterExhausted,
  }
}

fn transfer_kind(error: &InteropError) -> AppErrorKind {
  match error {
    InteropError::FileTooLarge => AppErrorKind::FileTooLarge,
    InteropError::UnrecognizedFormat(_) => AppErrorKind::UnrecognizedFormat,
    InteropError::UnsupportedVersion { .. } => AppErrorKind::UnsupportedFileVersion,
    InteropError::PasswordRequired => AppErrorKind::PasswordRequired,
    InteropError::WrongPasswordOrCorrupted => AppErrorKind::WrongPasswordOrCorrupted,
    InteropError::UnsupportedCredential => AppErrorKind::UnsupportedCredential,
    InteropError::TooManySlots => AppErrorKind::TooManySlots,
    InteropError::UnsupportedKdfParams { .. } => AppErrorKind::UnsupportedFileKdfParams,
    InteropError::Malformed(_) => AppErrorKind::MalformedFile,
    InteropError::Randomness(_) => AppErrorKind::Randomness,
    InteropError::Encrypt(_) => AppErrorKind::Encryption,
    InteropError::Serialize(_) => AppErrorKind::ExportSerialize,
  }
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};

  use super::*;

  fn wire(error: &AppError) -> Value {
    serde_json::to_value(error).unwrap()
  }

  #[test]
  fn errors_cross_ipc_as_a_kind_and_a_message() {
    assert_eq!(
      wire(&AppError::Locked),
      json!({ "kind": "Locked", "message": "the vault is locked" })
    );
    assert_eq!(
      wire(&AppError::WeakPassword),
      json!({
        "kind": "WeakPassword",
        "message": "the password must be at least 12 characters long",
      })
    );
  }

  #[test]
  fn nested_errors_get_the_kind_of_their_innermost_variant() {
    assert_eq!(
      AppError::from(StorageError::Crypto(CryptoError::Decrypt)).kind(),
      AppErrorKind::WrongPassword
    );
    assert_eq!(
      AppError::from(StorageError::VaultCore(VaultCoreError::BadMagic)).kind(),
      AppErrorKind::NotAVault
    );
    assert_eq!(
      AppError::from(QrError::InvalidOtpConfig(OtpError::CounterExhausted)).kind(),
      AppErrorKind::CounterExhausted
    );
    assert_eq!(
      AppError::from(InteropError::PasswordRequired).kind(),
      AppErrorKind::PasswordRequired
    );
    assert_eq!(
      AppError::from(InvalidSetting::ClipboardClearSeconds(5)).kind(),
      AppErrorKind::InvalidClipboardClearSeconds
    );
  }

  #[test]
  fn the_message_includes_every_cause() {
    let error = AppError::FileWrite {
      path: PathBuf::from("/backups/aegis.json"),
      source: io::Error::new(io::ErrorKind::StorageFull, "No space left on device"),
    };
    assert_eq!(
      wire(&error),
      json!({
        "kind": "FileWrite",
        "message": "could not write /backups/aegis.json: No space left on device",
      })
    );

    let error = AppError::from(StorageError::Io {
      path: PathBuf::from("/data/vault"),
      source: io::Error::other("Permission denied"),
    });
    assert_eq!(
      error.message(),
      "file system operation failed on /data/vault: Permission denied"
    );
  }

  #[test]
  fn every_kind_serializes_as_its_name() {
    let kinds = [
      (AppErrorKind::Locked, "Locked"),
      (AppErrorKind::VaultInUse, "VaultInUse"),
      (
        AppErrorKind::WrongPasswordOrCorrupted,
        "WrongPasswordOrCorrupted",
      ),
      (
        AppErrorKind::UnsupportedFileKdfParams,
        "UnsupportedFileKdfParams",
      ),
      (
        AppErrorKind::InvalidClipboardClearSeconds,
        "InvalidClipboardClearSeconds",
      ),
    ];
    for (kind, name) in kinds {
      assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
    }
  }
}
