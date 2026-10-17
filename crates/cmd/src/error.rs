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
use vault_sync::{
  devices::DeviceSetError, file::FileError, gatt::LinkError, identity::IdentityError,
  invitation::InvitationError, protocol::ProtocolError, session::SessionError,
};

use crate::{
  biometric::SealError, clipboard::ClipboardError, diagnostics::Severity,
  password::MIN_PASSWORD_CHARS,
};

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
  #[error("there is no diagnostic log on this device")]
  NoDiagnosticLog,
  #[error("no diagnostic report is waiting to be saved; prepare it again")]
  NoDiagnosticReport,
  #[error("could not put the diagnostic report together")]
  DiagnosticReport(#[from] zip::result::ZipError),
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
  OpenLink(#[source] crate::about::OpenLinkError),
  /// The file dialog could not be shown.
  #[error("could not open the file dialog")]
  FileDialog(#[source] tauri::Error),
  /// A file picked for import is neither an Aegis vault export nor a 2FAS
  /// backup, going by its content (its name is not looked at).
  #[error("{0} is neither an Aegis vault export nor a 2FAS backup")]
  UnsupportedFileType(PathBuf),
  /// Biometric unlock could not seal or unseal the vault key.
  #[error(transparent)]
  Biometric(#[from] SealError),
  /// Unlocking with a biometric, but it was never turned on (or was
  /// turned off).
  #[error("biometric unlock is not turned on")]
  BiometricNotEnabled,
  #[error("the system clock is set before 1970")]
  SystemClock(#[source] SystemTimeError),
  #[error("a background task failed")]
  BackgroundTask(#[source] tauri::Error),
  /// This device's sync identity is not set up, or cannot be listed in
  /// the device set.
  #[error(transparent)]
  Identity(#[from] IdentityError),
  /// No key could be generated for this device's sync identity.
  #[error("could not generate a device key")]
  KeyGeneration(#[from] snow::Error),
  /// A sync invitation could not be opened or read.
  #[error(transparent)]
  Invitation(#[from] InvitationError),
  /// A sync or join file could not be written or read.
  #[error(transparent)]
  SyncFile(#[from] FileError),
  /// The device set refused a change.
  #[error(transparent)]
  Devices(#[from] DeviceSetError),
  #[error("'{0}' is not a valid device id")]
  InvalidDeviceId(String),
  #[error("this device is not paired with your vault")]
  UntrustedDevice,
  #[error(
    "this device is not paired with any other yet: pair them first, with \
     \u{201c}Pair a new device\u{201d} on one and \u{201c}Join with an invitation\u{201d} on the other"
  )]
  NothingPaired,
  #[error("the vault does not sync with this device")]
  UnknownDevice,
  /// A live sync or pairing session failed.
  #[error(transparent)]
  Link(#[from] LinkError),
  #[error("the other device disconnected before the sync finished")]
  Disconnected,
  /// The Bluetooth adapter or daemon failed.
  #[cfg(target_os = "linux")]
  #[error(transparent)]
  Bluetooth(ble_linux::BleError),
  /// The Bluetooth plugin failed. Its own message is for the diagnostic
  /// log, not for people: they get what to do about it.
  #[cfg(target_os = "android")]
  #[error("Bluetooth did not work: keep both devices close together and try again")]
  Blec(tauri_plugin_blec::Error),
  /// The Bluetooth peripheral failed. Its own message is for the
  /// diagnostic log, as with [`Self::Blec`].
  #[cfg(target_os = "android")]
  #[error("Bluetooth did not work: keep both devices close together and try again")]
  Peripheral(ble_android::Error),
  #[error("Bluetooth is off: turn it on and try again")]
  BluetoothOff,
  #[error(
    "this phone cannot wait for other devices over Bluetooth: let the other device wait \
     instead, or use a sync file"
  )]
  CannotWait,
  #[error("Bluetooth sync is not available on this device yet")]
  BluetoothUnsupported,
  #[error(
    "Arsu may not use Bluetooth: allow Nearby devices (Location before Android 12) in the \
     system settings"
  )]
  BluetoothPermissionDenied,
  #[error(
    "the device showing this invitation was not found nearby: keep its invitation open and \
     the two devices close together, or read its QR code and save a join file instead"
  )]
  InvitingDeviceNotFound,
  #[error("this device cannot remove itself: remove it from another device")]
  CannotRemoveThisDevice,
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
  NoDiagnosticLog,
  NoDiagnosticReport,
  DiagnosticReport,
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
  // Biometric unlock.
  BiometricCancelled,
  BiometricLockout,
  BiometricInvalidated,
  BiometricUnavailable,
  BiometricFailed,
  BiometricNotEnabled,
  // The runtime.
  SystemClock,
  BackgroundTask,
  // Sync.
  TooManyDevices,
  DeviceRemoved,
  InvalidDeviceId,
  UntrustedDevice,
  NothingPaired,
  UnknownDevice,
  CannotRemoveThisDevice,
  SyncNotSetUp,
  NotAnInvitation,
  UnsupportedInvitationVersion,
  MistypedInvitationCode,
  NotASyncFile,
  UnsupportedSyncFileVersion,
  UnsupportedSyncProtocol,
  SyncFileForAnotherDevice,
  NoMatchingInvitation,
  OwnSyncFile,
  DamagedSyncFile,
  SyncHandshakeFailed,
  UnexpectedDevice,
  SameDevice,
  SyncProtocolError,
  BluetoothTransfer,
  BluetoothDisconnected,
  BluetoothUnavailable,
  BluetoothCannotWait,
  BluetoothUnsupported,
  BluetoothPermissionDenied,
  InvitingDeviceNotFound,
}

impl AppErrorKind {
  /// How bad a failure of this kind is, for the diagnostic log; `None` for an expected outcome, such
  /// as a wrong password or a mistyped field, which is not logged.
  #[must_use]
  pub const fn severity(self) -> Option<Severity> {
    use Severity::{Critical, Error, Warning};
    match self {
      // What the user typed or chose: the interface already says so.
      Self::Locked
      | Self::VaultNotFound
      | Self::VaultAlreadyExists
      | Self::WrongPassword
      | Self::WeakPassword
      | Self::InvalidAutoLockMinutes
      | Self::InvalidClipboardClearSeconds
      | Self::EntryNotFound
      | Self::InvalidEntryId
      | Self::NotOtpauth
      | Self::MissingLabel
      | Self::LabelContainsColon
      | Self::InvalidLabelEncoding
      | Self::MissingSecret
      | Self::InvalidSecret
      | Self::MissingCounter
      | Self::InvalidNumber
      | Self::UnknownOtpType
      | Self::UnknownAlgorithm
      | Self::InvalidDigits
      | Self::InvalidPeriod
      | Self::EmptySecret
      | Self::InvalidCode
      | Self::NoPendingImport
      | Self::NoDiagnosticLog
      | Self::NoDiagnosticReport
      | Self::PasswordRequired
      | Self::WrongPasswordOrCorrupted
      | Self::BiometricCancelled
      | Self::BiometricNotEnabled
      | Self::InvalidDeviceId
      | Self::NothingPaired
      | Self::CannotRemoveThisDevice
      | Self::NotAnInvitation
      | Self::MistypedInvitationCode
      | Self::NotASyncFile => None,
      // Outside Arsu: the device, its settings, another app's file,
      // another device.
      Self::VaultInUse
      | Self::InvalidSettingsFile
      | Self::CounterExhausted
      | Self::NotAFile
      | Self::InvalidFilePath
      | Self::OpenLink
      | Self::UnsupportedFileType
      | Self::FileTooLarge
      | Self::UnrecognizedFormat
      | Self::UnsupportedFileVersion
      | Self::UnsupportedCredential
      | Self::TooManySlots
      | Self::UnsupportedFileKdfParams
      | Self::MalformedFile
      | Self::BiometricLockout
      | Self::BiometricInvalidated
      | Self::BiometricUnavailable
      | Self::TooManyDevices
      | Self::DeviceRemoved
      | Self::UntrustedDevice
      | Self::UnknownDevice
      | Self::SyncNotSetUp
      | Self::UnsupportedInvitationVersion
      | Self::UnsupportedSyncFileVersion
      | Self::UnsupportedSyncProtocol
      | Self::SyncFileForAnotherDevice
      | Self::NoMatchingInvitation
      | Self::OwnSyncFile
      | Self::UnexpectedDevice
      | Self::SameDevice
      | Self::BluetoothDisconnected
      | Self::BluetoothUnavailable
      | Self::BluetoothCannotWait
      | Self::BluetoothUnsupported
      | Self::BluetoothPermissionDenied
      | Self::InvitingDeviceNotFound => Some(Warning),
      // Arsu failed at what it was asked.
      Self::UnsupportedVaultVersion
      | Self::UnsupportedKdfParams
      | Self::KeyDerivation
      | Self::StorageIo
      | Self::QrEncode
      | Self::Clipboard
      | Self::FileRead
      | Self::FileWrite
      | Self::FileDialog
      | Self::DiagnosticReport
      | Self::ExportSerialize
      | Self::BiometricFailed
      | Self::SystemClock
      | Self::DamagedSyncFile
      | Self::SyncHandshakeFailed
      | Self::SyncProtocolError
      | Self::BluetoothTransfer => Some(Error),
      // The app or the vault at risk.
      Self::NotAVault
      | Self::VaultTruncated
      | Self::VaultEncode
      | Self::VaultDecode
      | Self::Encryption
      | Self::Randomness
      | Self::NoProjectDirs
      | Self::BackgroundTask => Some(Critical),
    }
  }
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
      Self::NoDiagnosticLog => AppErrorKind::NoDiagnosticLog,
      Self::NoDiagnosticReport => AppErrorKind::NoDiagnosticReport,
      Self::DiagnosticReport(_) => AppErrorKind::DiagnosticReport,
      Self::NotAFile(_) => AppErrorKind::NotAFile,
      Self::FileRead { .. } => AppErrorKind::FileRead,
      Self::FileWrite { .. } => AppErrorKind::FileWrite,
      Self::InvalidFilePath => AppErrorKind::InvalidFilePath,
      Self::FileDialog(_) => AppErrorKind::FileDialog,
      Self::OpenLink(_) => AppErrorKind::OpenLink,
      Self::UnsupportedFileType(_) => AppErrorKind::UnsupportedFileType,
      Self::Biometric(error) => match error {
        SealError::Cancelled => AppErrorKind::BiometricCancelled,
        SealError::Lockout => AppErrorKind::BiometricLockout,
        SealError::Invalidated => AppErrorKind::BiometricInvalidated,
        SealError::Unavailable => AppErrorKind::BiometricUnavailable,
        SealError::Failed(_) => AppErrorKind::BiometricFailed,
      },
      Self::BiometricNotEnabled => AppErrorKind::BiometricNotEnabled,
      Self::SystemClock(_) => AppErrorKind::SystemClock,
      Self::BackgroundTask(_) => AppErrorKind::BackgroundTask,
      Self::Identity(IdentityError::NoIdentity) => AppErrorKind::SyncNotSetUp,
      Self::Identity(IdentityError::Devices(error)) | Self::Devices(error) => devices_kind(*error),
      Self::KeyGeneration(_) => AppErrorKind::Randomness,
      Self::Invitation(error) => invitation_kind(*error),
      Self::SyncFile(error) => sync_file_kind(error),
      Self::InvalidDeviceId(_) => AppErrorKind::InvalidDeviceId,
      Self::UntrustedDevice => AppErrorKind::UntrustedDevice,
      Self::NothingPaired => AppErrorKind::NothingPaired,
      Self::UnknownDevice => AppErrorKind::UnknownDevice,
      Self::Link(LinkError::Gatt(_)) => AppErrorKind::BluetoothTransfer,
      Self::Link(LinkError::Session(error)) => session_kind(error),
      Self::Disconnected => AppErrorKind::BluetoothDisconnected,
      #[cfg(target_os = "linux")]
      Self::Bluetooth(_) => AppErrorKind::BluetoothUnavailable,
      #[cfg(target_os = "android")]
      Self::Blec(_) => AppErrorKind::BluetoothUnavailable,
      #[cfg(target_os = "android")]
      Self::Peripheral(_) => AppErrorKind::BluetoothUnavailable,
      Self::BluetoothOff => AppErrorKind::BluetoothUnavailable,
      Self::CannotWait => AppErrorKind::BluetoothCannotWait,
      Self::BluetoothUnsupported => AppErrorKind::BluetoothUnsupported,
      Self::BluetoothPermissionDenied => AppErrorKind::BluetoothPermissionDenied,
      Self::InvitingDeviceNotFound => AppErrorKind::InvitingDeviceNotFound,
      Self::CannotRemoveThisDevice => AppErrorKind::CannotRemoveThisDevice,
    }
  }

  /// The error followed by each of its causes (see [`describe`]).
  #[must_use]
  pub fn message(&self) -> String {
    describe(self)
  }

  /// The cause a plain message leaves out for people, for the diagnostic
  /// log: a Bluetooth plugin's own error.
  #[must_use]
  pub fn hidden_cause(&self) -> Option<String> {
    match self {
      #[cfg(target_os = "android")]
      Self::Blec(error) => Some(format!("{error:?}")),
      #[cfg(target_os = "android")]
      Self::Peripheral(error) => Some(format!("{error:?}")),
      _ => None,
    }
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

fn invitation_kind(error: InvitationError) -> AppErrorKind {
  match error {
    InvitationError::NotAnInvitation => AppErrorKind::NotAnInvitation,
    InvitationError::UnsupportedVersion => AppErrorKind::UnsupportedInvitationVersion,
    InvitationError::BadChecksum => AppErrorKind::MistypedInvitationCode,
    InvitationError::NoIdentity => AppErrorKind::SyncNotSetUp,
    InvitationError::Random => AppErrorKind::Randomness,
  }
}

fn sync_file_kind(error: &FileError) -> AppErrorKind {
  match error {
    FileError::NotASyncFile => AppErrorKind::NotASyncFile,
    FileError::UnsupportedVersion(_) => AppErrorKind::UnsupportedSyncFileVersion,
    FileError::NotForThisDevice => AppErrorKind::SyncFileForAnotherDevice,
    FileError::NoInvitation => AppErrorKind::NoMatchingInvitation,
    FileError::UntrustedSender(_) => AppErrorKind::UntrustedDevice,
    FileError::OwnFile => AppErrorKind::OwnSyncFile,
    FileError::Noise(_) => AppErrorKind::Encryption,
    FileError::Protocol(ProtocolError::UnsupportedProtocol(_)) => {
      AppErrorKind::UnsupportedSyncProtocol
    }
    FileError::Damaged
    | FileError::Wire(_)
    | FileError::Protocol(ProtocolError::Cbor(_) | ProtocolError::TooManyTombstones) => {
      AppErrorKind::DamagedSyncFile
    }
  }
}

fn session_kind(error: &SessionError) -> AppErrorKind {
  match error {
    SessionError::Noise(_) => AppErrorKind::SyncHandshakeFailed,
    SessionError::UntrustedPeer(_) => AppErrorKind::UntrustedDevice,
    SessionError::UnexpectedPeer(_) => AppErrorKind::UnexpectedDevice,
    SessionError::OwnKey => AppErrorKind::SameDevice,
    SessionError::Protocol(ProtocolError::UnsupportedProtocol(_)) => {
      AppErrorKind::UnsupportedSyncProtocol
    }
    SessionError::Wire(_)
    | SessionError::Protocol(ProtocolError::Cbor(_) | ProtocolError::TooManyTombstones)
    | SessionError::UnexpectedMessage
    | SessionError::Closed => AppErrorKind::SyncProtocolError,
  }
}

fn devices_kind(error: DeviceSetError) -> AppErrorKind {
  match error {
    DeviceSetError::TooManyDevices => AppErrorKind::TooManyDevices,
    DeviceSetError::Removed => AppErrorKind::DeviceRemoved,
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

  #[test]
  fn device_set_errors_have_kinds_of_their_own() {
    assert_eq!(
      AppError::from(IdentityError::Devices(DeviceSetError::TooManyDevices)).kind(),
      AppErrorKind::TooManyDevices
    );
    assert_eq!(
      AppError::from(IdentityError::Devices(DeviceSetError::Removed)).kind(),
      AppErrorKind::DeviceRemoved
    );
  }
}
