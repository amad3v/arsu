//! The `#[tauri::command]` handlers: everything the frontend can call.
//! The wire types are in [`crate::dto`], and the errors in
//! [`crate::error`]: each command's `# Errors` names the
//! [`AppErrorKind`](crate::error::AppErrorKind)s it can fail with, less
//! two cases every list leaves out:
//! - any command can fail with `BackgroundTask`;
//! - a command that saves the vault can fail with a *vault-write kind*
//!   (`StorageIo`, `VaultInUse`, `Encryption`, `Randomness`,
//!   `VaultEncode`), and then changes nothing: the entries, counters and
//!   settings in memory are as they were before the call.
//!
//! Each command only moves its arguments into [`AppState`], where the
//! logic lives, and runs on Tauri's blocking thread pool. Tauri would run
//! a plain (non-`async`) command on the main thread, where the KDF, file
//! I/O or waiting for a lock freezes the window — and where a native
//! dialog's `blocking_*` call deadlocks, since it waits for the main
//! thread itself.

use std::{thread, time::Duration};

use base64::{Engine, prelude::BASE64_STANDARD};
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::{
  about,
  biometric::{self, PlatformSealer},
  bluetooth::{self, Bluetooth},
  clipboard::{self, ClipboardExt, CopyTicket, OtpCode, SystemClipboard},
  clock::unix_now,
  diagnostics::{self, Diagnostics},
  dto::{
    AboutInfo, AppLink, BiometricStatus, CodeResponse, DeviceCheck, DiagnosticReportPreview,
    EntrySummary, ImportSummary, ManualEntryInput, NearbyDevice, PickedFile, SecretString,
    SecretText, Settings, SettingsUpdate, SyncInvitation, SyncOverview, SyncSummary, UiError,
    parse_entry_id,
  },
  error::AppError,
  file_dialog::{FileDialog, FileType, Purpose},
  files::UserFile,
  state::{AppState, labels},
};

/// Runs `task` on the blocking thread pool.
async fn run_blocking<T, F>(task: F) -> Result<T, AppError>
where
  F: FnOnce() -> Result<T, AppError> + Send + 'static,
  T: Send + 'static,
{
  tauri::async_runtime::spawn_blocking(task)
    .await
    .map_err(AppError::BackgroundTask)
    .and_then(|outcome| outcome)
    .inspect_err(|error| diagnostics::record!("command failed", error))
}

/// Runs `task` with the app state on the blocking thread pool.
async fn with_state<T, F>(app: AppHandle, task: F) -> Result<T, AppError>
where
  F: FnOnce(&AppState) -> Result<T, AppError> + Send + 'static,
  T: Send + 'static,
{
  run_blocking(move || task(&app.state::<AppState>())).await
}

// ---- the vault ---------------------------------------------------------

/// Whether a vault exists, to choose between the create and unlock
/// screens.
///
/// # Errors
///
/// `VaultInUse` (another instance has the vault open; retrying succeeds once it quits) or `StorageIo`.
#[tauri::command]
pub async fn vault_exists(app: AppHandle) -> Result<bool, AppError> {
  with_state(app, AppState::vault_exists).await
}

/// Creates the vault and unlocks it (Argon2id; about a second).
///
/// # Errors
///
/// `WeakPassword`, `VaultAlreadyExists`, `KeyDerivation`, or a vault-write kind.
#[tauri::command]
pub async fn create_vault(app: AppHandle, master_password: SecretString) -> Result<(), AppError> {
  with_state(app, move |state| state.create_vault(&master_password)).await
}

/// Unlocks the vault (Argon2id; about a second).
///
/// # Errors
///
/// `WrongPassword`, `VaultNotFound`, `NotAVault`, `VaultTruncated`,
/// `UnsupportedVaultVersion`, `UnsupportedKdfParams`, `VaultDecode`,
/// `KeyDerivation`, `VaultInUse`, `StorageIo`, or a vault-write kind (from
/// upgrading a vault's key-derivation parameters).
#[tauri::command]
pub async fn unlock_vault(app: AppHandle, master_password: SecretString) -> Result<(), AppError> {
  with_state(app, move |state| state.unlock_vault(&master_password)).await
}

/// Locks the vault: the key and every decrypted seed leave memory.
///
/// # Errors
///
/// Only `BackgroundTask`.
#[tauri::command]
pub async fn lock_vault(app: AppHandle) -> Result<(), AppError> {
  with_state(app, |state| {
    state.lock_vault();
    Ok(())
  })
  .await
}

/// # Errors
///
/// Only `BackgroundTask`.
#[tauri::command]
pub async fn is_unlocked(app: AppHandle) -> Result<bool, AppError> {
  with_state(app, |state| Ok(state.is_unlocked())).await
}

/// Tells the backend the user is active, postponing the auto-lock.
///
/// # Errors
///
/// Only `BackgroundTask`.
#[tauri::command]
pub async fn record_activity(app: AppHandle) -> Result<(), AppError> {
  with_state(app, |state| {
    state.record_activity();
    Ok(())
  })
  .await
}

// ---- the device ----------------------------------------------------------

/// Whether the phone looks rooted, and whether the user has accepted the
/// risk, to warn them before the vault is opened there.
///
/// # Errors
///
/// `FileRead` if the consent cannot be looked for.
#[tauri::command]
pub async fn device_check(app: AppHandle) -> Result<DeviceCheck, AppError> {
  with_state(app, AppState::device_check).await
}

/// The user's answer to the rooted-phone warning: if they accept the
/// risk, it is recorded and not asked again; if not, the app closes.
///
/// # Errors
///
/// `FileWrite` if the consent cannot be stored.
#[tauri::command]
pub async fn answer_root_warning(window: WebviewWindow, accept: bool) -> Result<(), AppError> {
  if !accept {
    close_app(&window);
    return Ok(());
  }
  with_state(window.app_handle().clone(), AppState::accept_root_risk).await
}

/// Closes the app as its user would. On Android that is finishing the
/// activity, and removing it from the recent apps: Tauri's `exit` only
/// stops the event loop there, leaving the activity on screen with a
/// dead page. A window already gone has nothing left to close.
fn close_app(window: &WebviewWindow) {
  #[cfg(target_os = "android")]
  let _ = window.with_webview(|webview| {
    webview.jni_handle().exec(|env, activity, _webview| {
      if env
        .call_method(activity, "finishAndRemoveTask", "()V", &[])
        .is_err()
      {
        // Only an exception, which must not be left pending; the app stays open.
        let _ = env.exception_clear();
      }
    });
  });
  #[cfg(not(target_os = "android"))]
  window.app_handle().exit(0);
}

// ---- biometric unlock ----------------------------------------------------

/// Whether biometric unlock is supported here (Android), usable now, and
/// turned on.
///
/// # Errors
///
/// `FileRead` if whether it is on cannot be told.
#[tauri::command]
pub async fn biometric_status(app: AppHandle) -> Result<BiometricStatus, AppError> {
  run_blocking(move || {
    let availability = biometric::availability(&app);
    Ok(BiometricStatus {
      supported: availability.supported,
      available: availability.available,
      reason: availability.reason,
      enabled: app.state::<AppState>().biometric_enabled()?,
    })
  })
  .await
}

/// Turns biometric unlock on, after checking the master password again;
/// the system asks for the user's fingerprint or face.
///
/// # Errors
///
/// `Locked`, `WrongPassword`, `KeyDerivation`, `BiometricCancelled`,
/// `BiometricLockout`, `BiometricUnavailable`, `BiometricFailed` or
/// `FileWrite`.
#[tauri::command]
pub async fn enable_biometric_unlock(
  app: AppHandle,
  master_password: SecretString,
) -> Result<(), AppError> {
  run_blocking(move || {
    app
      .state::<AppState>()
      .enable_biometric_unlock(&master_password, &PlatformSealer::new(&app))
  })
  .await
}

/// Turns biometric unlock off.
///
/// # Errors
///
/// `FileWrite` or `BiometricFailed`.
#[tauri::command]
pub async fn disable_biometric_unlock(app: AppHandle) -> Result<(), AppError> {
  run_blocking(move || {
    app
      .state::<AppState>()
      .disable_biometric_unlock(&PlatformSealer::new(&app))
  })
  .await
}

/// Unlocks the vault with the user's fingerprint or face.
///
/// # Errors
///
/// `BiometricNotEnabled`, `BiometricCancelled`, `BiometricLockout`,
/// `BiometricInvalidated` (it was turned off: the phone's biometrics or
/// the vault changed), `BiometricUnavailable`, `BiometricFailed`,
/// `FileRead`, or the vault's own unlock errors.
#[tauri::command]
pub async fn unlock_with_biometric(app: AppHandle) -> Result<(), AppError> {
  run_blocking(move || {
    app
      .state::<AppState>()
      .unlock_with_biometric(&PlatformSealer::new(&app))
  })
  .await
}

// ---- entries -----------------------------------------------------------

/// # Errors
///
/// `Locked`.
#[tauri::command]
pub async fn list_entries(app: AppHandle) -> Result<Vec<EntrySummary>, AppError> {
  with_state(app, AppState::list_entries).await
}

/// The current code of one entry. For HOTP this advances and saves the
/// counter.
///
/// # Errors
///
/// `Locked`, `EntryNotFound`, `InvalidEntryId` or `SystemClock`; for HOTP
/// also `CounterExhausted` or a vault-write kind.
#[tauri::command]
pub async fn get_current_code(app: AppHandle, entry_id: String) -> Result<CodeResponse, AppError> {
  with_state(app, move |state| {
    state.current_code(parse_entry_id(&entry_id)?, unix_now()?)
  })
  .await
}

/// Copies a code to the clipboard and clears it again after the
/// configured delay, if it is still there.
///
/// # Errors
///
/// `InvalidCode` or `Clipboard`.
#[tauri::command]
pub async fn copy_code(app: AppHandle, code: String) -> Result<(), AppError> {
  run_blocking(move || {
    let code = OtpCode::parse(code)?;
    let state = app.state::<AppState>();
    state.record_activity();
    app
      .clipboard()
      .write_text(code.as_str())
      .map_err(AppError::Clipboard)?;
    let ticket = state.copied_codes().record(&code);
    clear_clipboard_later(app.clone(), ticket, state.clipboard_clear_after());
    Ok(())
  })
  .await
}

/// After `delay`, clears the clipboard if it still holds the code copied
/// with `ticket` (see [`clipboard::clear_if_unchanged`]).
fn clear_clipboard_later(app: AppHandle, ticket: CopyTicket, delay: Duration) {
  tauri::async_runtime::spawn_blocking(move || {
    thread::sleep(delay);
    // Best effort, and nobody is waiting for the result: if clearing
    // fails, the code stays until something else is copied.
    let _ = clipboard::clear_if_unchanged(
      app.clipboard(),
      app.state::<AppState>().copied_codes(),
      ticket,
    );
  });
}

/// Adds the account an `otpauth://` URI describes. Returns its id.
///
/// # Errors
///
/// The URI's own errors (`NotOtpauth`, `MissingLabel`,
/// `LabelContainsColon`, `InvalidLabelEncoding`, `MissingSecret`,
/// `InvalidSecret`, `MissingCounter`, `InvalidNumber`, `UnknownOtpType`,
/// `UnknownAlgorithm`, `InvalidDigits`, `InvalidPeriod`, `EmptySecret`),
/// `CounterExhausted`, `Locked`, `SystemClock`, or a vault-write kind.
#[tauri::command]
pub async fn add_entry_from_uri(app: AppHandle, uri: SecretString) -> Result<String, AppError> {
  with_state(app, move |state| {
    Ok(state.add_entry_from_uri(&uri)?.to_string())
  })
  .await
}

/// Adds a manually entered account. Returns its id.
///
/// # Errors
///
/// `MissingLabel` or `LabelContainsColon` (issuer and account),
/// `InvalidSecret` or `EmptySecret` (secret), `InvalidDigits`,
/// `InvalidPeriod`, `CounterExhausted`, `Locked`, `SystemClock`, or a
/// vault-write kind.
#[tauri::command]
pub async fn add_entry_manual(app: AppHandle, input: ManualEntryInput) -> Result<String, AppError> {
  with_state(app, move |state| {
    Ok(state.add_entry_manual(input)?.to_string())
  })
  .await
}

/// # Errors
///
/// `Locked`, `EntryNotFound`, `InvalidEntryId`, `SystemClock`, or a
/// vault-write kind (the entry then stays).
#[tauri::command]
pub async fn delete_entry(app: AppHandle, entry_id: String) -> Result<(), AppError> {
  with_state(app, move |state| {
    state.delete_entry(parse_entry_id(&entry_id)?)
  })
  .await
}

/// One entry's QR code, after re-checking the master password.
///
/// # Errors
///
/// `WrongPassword`, `Locked`, `EntryNotFound`, `InvalidEntryId`,
/// `KeyDerivation` or `QrEncode`.
#[tauri::command]
pub async fn export_entry_qr(
  app: AppHandle,
  entry_id: String,
  master_password: SecretString,
) -> Result<SecretText, AppError> {
  with_state(app, move |state| {
    state.export_entry_qr(parse_entry_id(&entry_id)?, &master_password)
  })
  .await
}

/// Shows an entry as a QR code once the user passes the fingerprint or
/// face check, in place of the master password.
///
/// # Errors
///
/// `InvalidEntryId`, `Locked`, `EntryNotFound`, `BiometricNotEnabled`,
/// `BiometricCancelled`, `BiometricLockout`, `BiometricInvalidated`,
/// `BiometricUnavailable`, `BiometricFailed`, `FileRead` or `QrEncode`.
#[tauri::command]
pub async fn export_entry_qr_with_biometric(
  app: AppHandle,
  entry_id: String,
) -> Result<SecretText, AppError> {
  run_blocking(move || {
    app
      .state::<AppState>()
      .export_entry_qr_with_biometric(parse_entry_id(&entry_id)?, &PlatformSealer::new(&app))
  })
  .await
}

// ---- import and export -------------------------------------------------

/// The backups the import dialog offers: either app's, then each one's,
/// then any file. The names are only a convenience: a backup is told apart
/// by its content, so one saved under another name still imports.
const IMPORT_FILE_TYPES: &[FileType] = &[
  FileType {
    name: "Aegis or 2FAS backup (*.json, *.2fas)",
    patterns: &["*.json", "*.2fas"],
    mime_types: &[],
  },
  AEGIS_FILE_TYPE,
  FileType {
    name: "2FAS backup (*.2fas)",
    patterns: &["*.2fas"],
    mime_types: &[],
  },
  FileType {
    name: "All files",
    patterns: &["*"],
    mime_types: &[],
  },
];

/// An Aegis vault export: what the import dialog offers, and the export
/// writes.
const AEGIS_FILE_TYPE: FileType = FileType {
  name: "Aegis backup (*.json)",
  patterns: &["*.json"],
  mime_types: &[],
};

/// Asks the user for a backup file to import, Aegis' or 2FAS', and reads
/// it to tell which, by its content. Returns `None` if they cancel.
///
/// # Errors
///
/// `FileDialog`, `InvalidFilePath`, `NotAFile`, `FileRead`,
/// `FileTooLarge`, or `UnsupportedFileType` (neither app's backup).
#[tauri::command]
pub async fn pick_import_file(window: WebviewWindow) -> Result<Option<PickedFile>, AppError> {
  run_blocking(move || {
    let dialog = FileDialog {
      title: "Import a backup",
      purpose: Purpose::Open,
      file_types: IMPORT_FILE_TYPES,
    };
    let Some(file) = dialog.show(&window)? else {
      return Ok(None);
    };
    window.state::<AppState>().pick_import(file).map(Some)
  })
  .await
}

/// Imports the picked file.
///
/// # Errors
///
/// `NoPendingImport`, `Locked`, `NotAFile`, `FileRead`, the file's own
/// errors (`FileTooLarge`, `UnrecognizedFormat`, `UnsupportedFileVersion`,
/// `PasswordRequired`, `WrongPasswordOrCorrupted`, `UnsupportedCredential`,
/// `TooManySlots`, `UnsupportedFileKdfParams`, `MalformedFile`),
/// `SystemClock`, or a vault-write kind.
#[tauri::command]
pub async fn import_file(
  app: AppHandle,
  token: String,
  password: Option<SecretString>,
) -> Result<ImportSummary, AppError> {
  with_state(app, move |state| {
    state.import_file(&token, password.as_ref())
  })
  .await
}

/// Exports the vault as an encrypted Aegis file, to a location the user
/// chooses. Returns `false` if they cancel.
///
/// # Errors
///
/// `WeakPassword`, `Locked`, `FileDialog`, `InvalidFilePath`, `FileWrite`,
/// `Encryption`, `Randomness` or `ExportSerialize`.
#[tauri::command]
pub async fn export_to_aegis_file(
  window: WebviewWindow,
  password: SecretString,
) -> Result<bool, AppError> {
  run_blocking(move || {
    let state = window.state::<AppState>();
    state.check_export(&password)?;
    let dialog = FileDialog {
      title: "Export an encrypted backup",
      purpose: Purpose::Save {
        file_name: "aegis-export.json".into(),
      },
      file_types: &[AEGIS_FILE_TYPE],
    };
    let Some(file) = dialog.show(&window)? else {
      return Ok(false);
    };
    state.export_to_aegis(&password, &file)?;
    Ok(true)
  })
  .await
}

/// The images a QR code is read from: those the frontend's scanner
/// decodes (`qr-formats.ts`). Not SVG, which may have no pixel size.
const QR_IMAGE_FILE_TYPE: FileType = FileType {
  name: "Images (*.png, *.jpg, *.jpeg, *.gif, *.webp, *.bmp)",
  patterns: &[],
  mime_types: &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
  ],
};

/// The largest QR code image read: far above any screenshot's size.
const MAX_QR_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// Asks the user for an image of a QR code, and returns its bytes,
/// base64-encoded, for the frontend to scan. Returns `None` if they
/// cancel.
///
/// # Errors
///
/// `FileDialog`, `InvalidFilePath`, `NotAFile`, `FileRead` or
/// `FileTooLarge`.
#[tauri::command]
pub async fn pick_qr_image(window: WebviewWindow) -> Result<Option<String>, AppError> {
  run_blocking(move || {
    let dialog = FileDialog {
      title: "Choose a QR code image",
      purpose: Purpose::Open,
      file_types: &[QR_IMAGE_FILE_TYPE],
    };
    let Some(file) = dialog.show(&window)? else {
      return Ok(None);
    };
    window.state::<AppState>().record_activity();
    let image = file.read(MAX_QR_IMAGE_BYTES)?;
    Ok(Some(BASE64_STANDARD.encode(image.as_slice())))
  })
  .await
}

// ---- about ---------------------------------------------------------------

/// What the About dialog shows.
///
/// # Errors
///
/// Only `BackgroundTask`.
#[tauri::command]
pub async fn get_about_info(app: AppHandle) -> Result<AboutInfo, AppError> {
  run_blocking(move || Ok(AboutInfo::new(&app, &app.state::<AppState>()))).await
}

/// Copies the About dialog's details to the clipboard, for a bug report.
///
/// # Errors
///
/// `Clipboard`.
#[tauri::command]
pub async fn copy_about_details(app: AppHandle) -> Result<(), AppError> {
  run_blocking(move || {
    let state = app.state::<AppState>();
    state.record_activity();
    let details = AboutInfo::new(&app, &state).details_text();
    app
      .clipboard()
      .write_text(&details)
      .map_err(AppError::Clipboard)
  })
  .await
}

/// The diagnostic report's file type in the save dialog.
const REPORT_FILE_TYPES: &[FileType] = &[FileType {
  name: "Zip archive (*.zip)",
  patterns: &["*.zip"],
  mime_types: &["application/zip"],
}];

/// Prepares a diagnostic report (see [`crate::diagnostics`]) of the log
/// as it is now, and returns what it holds, for the user to see before
/// saving it ([`save_diagnostic_report`]).
///
/// # Errors
///
/// `NoDiagnosticLog` (the log could not be started on this device),
/// `FileRead` or `DiagnosticReport`.
#[tauri::command]
pub async fn prepare_diagnostic_report(
  app: AppHandle,
) -> Result<DiagnosticReportPreview, AppError> {
  run_blocking(move || {
    let diagnostics = app
      .try_state::<Diagnostics>()
      .ok_or(AppError::NoDiagnosticLog)?;
    let summary = app.state::<AppState>().diagnostic_summary();
    diagnostics.prepare_report(&summary, time::OffsetDateTime::now_utc())
  })
  .await
}

/// Saves the diagnostic report last prepared, exactly as the user saw it,
/// to a location they choose, for a bug report. Returns `false` if they
/// cancel.
///
/// # Errors
///
/// `NoDiagnosticReport`, `FileDialog`, `InvalidFilePath` or `FileWrite`.
#[tauri::command]
pub async fn save_diagnostic_report(window: WebviewWindow) -> Result<bool, AppError> {
  run_blocking(move || {
    let report = window
      .try_state::<Diagnostics>()
      .ok_or(AppError::NoDiagnosticReport)?
      .prepared_report()?;
    let dialog = FileDialog {
      title: "Save the diagnostic report",
      purpose: Purpose::Save {
        file_name: report.file_name.clone().into(),
      },
      file_types: REPORT_FILE_TYPES,
    };
    let Some(file) = dialog.show(&window)? else {
      return Ok(false);
    };
    file.write(&report.bytes)?;
    Ok(true)
  })
  .await
}

/// Logs an error the interface ran into, to the diagnostic log.
#[tauri::command]
#[expect(
  clippy::needless_pass_by_value,
  reason = "Tauri hands a command its arguments by value"
)]
pub fn report_ui_error(error: UiError) {
  diagnostics::record_ui_error(&error);
}

/// Opens one of the project's pages in the user's browser.
///
/// # Errors
///
/// `OpenLink` (no browser, or the portal refused).
#[tauri::command]
pub async fn open_link(window: WebviewWindow, link: AppLink) -> Result<(), AppError> {
  run_blocking(move || {
    window.state::<AppState>().record_activity();
    about::open_link(&window, link)
  })
  .await
}

// ---- the window --------------------------------------------------------

/// Shows the window, which starts hidden (`tauri.conf.json`). The
/// frontend calls this once it has rendered, so that its first frame,
/// already in the saved theme, is the first thing on screen instead of
/// the `WebView`'s blank white page. Best effort: if the window manager
/// refuses, the app shell's fallback tries again.
///
/// The one command not `async`: it runs on the main thread, where showing
/// a window has to happen anyway, and it doesn't wait on anything.
#[tauri::command]
#[expect(
  clippy::needless_pass_by_value,
  reason = "Tauri hands a command its window by value"
)]
pub fn show_window(window: WebviewWindow) {
  let _ = window.show();
}

// ---- sync --------------------------------------------------------------

/// This device and the other devices of the vault, setting sync up on
/// first use.
///
/// # Errors
///
/// `Locked`, `StorageIo`, `Randomness`, `TooManyDevices` or
/// `SystemClock`.
#[tauri::command]
pub async fn get_sync_overview(app: AppHandle) -> Result<SyncOverview, AppError> {
  with_state(app, AppState::sync_overview).await
}

/// Removes a trusted device from the vault, for good.
///
/// # Errors
///
/// `InvalidDeviceId`, `UnknownDevice`, `CannotRemoveThisDevice`,
/// `Locked`, `StorageIo`, `Randomness` or `SystemClock`.
#[tauri::command]
pub async fn remove_sync_device(app: AppHandle, device_id: String) -> Result<(), AppError> {
  with_state(app, move |state| state.remove_sync_device(&device_id)).await
}

/// What sync files are saved and opened as.
const SYNC_FILE_TYPES: &[FileType] = &[
  FileType {
    name: "Arsu sync file (*.arsu)",
    patterns: &["*.arsu"],
    mime_types: &[],
  },
  FileType {
    name: "All files",
    patterns: &["*"],
    mime_types: &[],
  },
];

/// Writes a sync file for a paired device, to a location the user
/// chooses. The device is checked before the dialog opens. Returns
/// `false` if they cancel.
///
/// # Errors
///
/// `InvalidDeviceId`, `UntrustedDevice`, `Locked`, `StorageIo`,
/// `Randomness`, `TooManyDevices`, `SystemClock`, `FileDialog`,
/// `InvalidFilePath`, `FileWrite`, `Encryption` or `DamagedSyncFile`.
#[tauri::command]
pub async fn export_sync_file(window: WebviewWindow, device_id: String) -> Result<bool, AppError> {
  run_blocking(move || {
    let dialog = FileDialog {
      title: "Save a sync file",
      purpose: Purpose::Save {
        file_name: "arsu-sync.arsu".into(),
      },
      file_types: SYNC_FILE_TYPES,
    };
    let state = window.state::<AppState>();
    state.check_sync_recipient(&device_id)?;
    let Some(file) = dialog.show(&window)? else {
      return Ok(false);
    };
    state.export_sync_file(&device_id, &file)?;
    Ok(true)
  })
  .await
}

/// Writes a join file for the device that showed an invitation, given
/// the text of its QR code, to a location the user chooses. The
/// invitation is checked before the dialog opens. Returns `false` if
/// they cancel.
///
/// # Errors
///
/// `NotAnInvitation`, `UnsupportedInvitationVersion`, `DeviceRemoved`,
/// `Locked`, `StorageIo`, `Randomness`, `TooManyDevices`, `SystemClock`,
/// `FileDialog`, `InvalidFilePath`, `FileWrite`, `Encryption` or
/// `DamagedSyncFile`.
#[tauri::command]
pub async fn export_join_file(
  window: WebviewWindow,
  invitation: SecretString,
) -> Result<bool, AppError> {
  run_blocking(move || {
    let dialog = FileDialog {
      title: "Save a join file",
      purpose: Purpose::Save {
        file_name: "arsu-join.arsu".into(),
      },
      file_types: SYNC_FILE_TYPES,
    };
    let state = window.state::<AppState>();
    state.check_join_invitation(&invitation)?;
    let Some(file) = dialog.show(&window)? else {
      return Ok(false);
    };
    state.export_join_file(&invitation, &file)?;
    Ok(true)
  })
  .await
}

/// Asks the user for a sync or join file, and merges it into the vault.
/// Returns `None` if they cancel.
///
/// # Errors
///
/// `FileDialog`, `InvalidFilePath`, `NotAFile`, `FileRead`,
/// `FileTooLarge`, `Locked`, `StorageIo`, `Randomness`, `SystemClock`,
/// `NotASyncFile`, `UnsupportedSyncFileVersion`, `UnsupportedSyncProtocol`,
/// `SyncFileForAnotherDevice`, `NoMatchingInvitation`, `UntrustedDevice`,
/// `OwnSyncFile`, `DamagedSyncFile`, `Encryption`, `TooManyDevices` or
/// `DeviceRemoved`.
#[tauri::command]
pub async fn import_sync_file(window: WebviewWindow) -> Result<Option<SyncSummary>, AppError> {
  run_blocking(move || {
    let dialog = FileDialog {
      title: "Open a sync file",
      purpose: Purpose::Open,
      file_types: SYNC_FILE_TYPES,
    };
    let Some(file) = dialog.show(&window)? else {
      return Ok(None);
    };
    window.state::<AppState>().import_sync_file(&file).map(Some)
  })
  .await
}

// ---- live sync over Bluetooth ------------------------------------------

/// Starts serving paired devices over Bluetooth, one after the other,
/// reporting each sync as a `sync-session` event, until
/// [`stop_bluetooth`]. A phone that cannot advertise reports so as the
/// first session's failure.
///
/// # Errors
///
/// `Locked`, or `BluetoothUnsupported` where this device has no
/// Bluetooth support at all. Later failures arrive as events.
#[tauri::command]
#[expect(
  clippy::needless_pass_by_value,
  reason = "Tauri hands a command its state by value"
)]
pub fn start_bluetooth_sync(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
  if !state.is_unlocked() {
    return Err(AppError::Locked);
  }
  #[cfg(any(target_os = "linux", target_os = "android"))]
  {
    bluetooth::start_sync(&app);
    Ok(())
  }
  #[cfg(not(any(target_os = "linux", target_os = "android")))]
  {
    let _ = app;
    Err(AppError::BluetoothUnsupported)
  }
}

/// Opens an invitation from this device (a QR code and a code to type,
/// valid for 15 minutes and for one pairing) and waits for the device
/// that reads it to pair over Bluetooth, reporting the outcome as a
/// `sync-session` event.
/// Returns the invitation to show: its QR code also serves a join file,
/// should Bluetooth fail.
///
/// # Errors
///
/// `Locked`, `StorageIo`, `Randomness`, `TooManyDevices`, `SystemClock`,
/// `QrEncode`, or `BluetoothUnsupported` where this device has no
/// Bluetooth support at all.
#[tauri::command]
pub async fn start_bluetooth_pairing(app: AppHandle) -> Result<SyncInvitation, AppError> {
  let (invitation, expires_at, shown) = with_state(app.clone(), |state| {
    let (invitation, expires_at) = state.create_invitation()?;
    let shown = SyncInvitation::new(&invitation, expires_at)?;
    Ok((invitation, expires_at, shown))
  })
  .await?;
  #[cfg(any(target_os = "linux", target_os = "android"))]
  {
    bluetooth::start_pairing(&app, invitation, expires_at);
    Ok(shown)
  }
  #[cfg(not(any(target_os = "linux", target_os = "android")))]
  {
    let _ = (invitation, expires_at, shown);
    Err(AppError::BluetoothUnsupported)
  }
}

/// The Arsu devices nearby, found by a short Bluetooth scan.
///
/// # Errors
///
/// `BluetoothUnavailable`, or `BluetoothUnsupported` where this device
/// cannot scan yet.
#[tauri::command]
pub async fn find_bluetooth_devices() -> Result<Vec<NearbyDevice>, AppError> {
  #[cfg(any(target_os = "linux", target_os = "android"))]
  {
    async { bluetooth::find_devices(&bluetooth::central()?).await }
      .await
      .inspect_err(|error| diagnostics::record!("scan failed", error))
  }
  #[cfg(not(any(target_os = "linux", target_os = "android")))]
  {
    Err(AppError::BluetoothUnsupported)
  }
}

/// Syncs with a paired device nearby, by its Bluetooth address.
///
/// # Errors
///
/// `InvalidDeviceId`, `Locked`, `BluetoothUnavailable`,
/// `BluetoothUnsupported`, `BluetoothDisconnected`, `BluetoothTransfer`,
/// `UntrustedDevice`, `SyncHandshakeFailed`, `SyncProtocolError`,
/// `UnsupportedSyncProtocol`, `TooManyDevices`, `StorageIo` or
/// `SystemClock`.
#[tauri::command]
pub async fn sync_over_bluetooth(app: AppHandle, address: String) -> Result<SyncSummary, AppError> {
  #[cfg(any(target_os = "linux", target_os = "android"))]
  {
    async { bluetooth::sync_with(&app.state::<AppState>(), &bluetooth::central()?, &address).await }
      .await
      .inspect_err(|error| {
        diagnostics::record!("session failed", error, "role" => labels::SYNC_CONNECTING);
      })
  }
  #[cfg(not(any(target_os = "linux", target_os = "android")))]
  {
    let _ = (app, address);
    Err(AppError::BluetoothUnsupported)
  }
}

/// Pairs with the device nearby that shows an invitation, given as the
/// text of its QR code or as the typed code.
///
/// # Errors
///
/// `NotAnInvitation`, `UnsupportedInvitationVersion`,
/// `MistypedInvitationCode`, `InvitingDeviceNotFound`, `UnexpectedDevice`,
/// and as [`sync_over_bluetooth`].
#[tauri::command]
pub async fn join_over_bluetooth(
  app: AppHandle,
  invitation: SecretString,
) -> Result<SyncSummary, AppError> {
  #[cfg(any(target_os = "linux", target_os = "android"))]
  {
    async {
      bluetooth::join(
        &app.state::<AppState>(),
        &bluetooth::central()?,
        invitation.expose(),
      )
      .await
    }
    .await
    .inspect_err(|error| {
      diagnostics::record!("session failed", error, "role" => labels::PAIRING_JOINING);
    })
  }
  #[cfg(not(any(target_os = "linux", target_os = "android")))]
  {
    let _ = (app, invitation);
    Err(AppError::BluetoothUnsupported)
  }
}

/// Stops Bluetooth sync or pairing: advertising ends, and a session in
/// progress is dropped without merging.
#[tauri::command]
#[expect(
  clippy::needless_pass_by_value,
  reason = "Tauri hands a command its state by value"
)]
pub fn stop_bluetooth(running: State<'_, Bluetooth>) {
  running.stop();
}

// ---- settings ----------------------------------------------------------

/// # Errors
///
/// `InvalidSettingsFile` or `StorageIo` if the settings file can't be
/// read; the defaults are in effect meanwhile.
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings, AppError> {
  with_state(app, AppState::settings).await
}

/// Changes some settings. Returns all of them.
///
/// # Errors
///
/// `InvalidAutoLockMinutes`, `InvalidClipboardClearSeconds` or
/// `StorageIo`.
#[tauri::command]
pub async fn update_settings(app: AppHandle, update: SettingsUpdate) -> Result<Settings, AppError> {
  with_state(app, move |state| state.update_settings(update)).await
}
