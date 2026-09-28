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
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::{
  about,
  clipboard::{self, ClipboardExt, CopyTicket, OtpCode, SystemClipboard},
  clock::unix_now,
  dto::{
    AboutInfo, AppLink, CodeResponse, EntrySummary, ImportSummary, ManualEntryInput, PickedFile,
    SecretQrSvg, SecretString, Settings, SettingsUpdate, parse_entry_id,
  },
  error::AppError,
  file_dialog::{FileDialog, FileType, Purpose},
  files::read_file,
  state::AppState,
};

/// Runs `task` on the blocking thread pool.
async fn run_blocking<T, F>(task: F) -> Result<T, AppError>
where
  F: FnOnce() -> Result<T, AppError> + Send + 'static,
  T: Send + 'static,
{
  tauri::async_runtime::spawn_blocking(task)
    .await
    .map_err(AppError::BackgroundTask)?
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
) -> Result<SecretQrSvg, AppError> {
  with_state(app, move |state| {
    state.export_entry_qr(parse_entry_id(&entry_id)?, &master_password)
  })
  .await
}

// ---- import and export -------------------------------------------------

/// The backups the import dialog offers: either app's, then each one's.
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
];

/// An Aegis vault export: what the import dialog offers, and the export
/// writes.
const AEGIS_FILE_TYPE: FileType = FileType {
  name: "Aegis backup (*.json)",
  patterns: &["*.json"],
  mime_types: &[],
};

/// Asks the user for a backup file to import, Aegis' or 2FAS'. Returns
/// `None` if they cancel.
///
/// # Errors
///
/// `FileDialog`, `InvalidFilePath`, or `UnsupportedFileType` (a name
/// typed into the dialog with another extension).
#[tauri::command]
pub async fn pick_import_file(window: WebviewWindow) -> Result<Option<PickedFile>, AppError> {
  run_blocking(move || {
    let dialog = FileDialog {
      title: "Import a backup",
      purpose: Purpose::Open,
      file_types: IMPORT_FILE_TYPES,
    };
    let Some(path) = dialog.show(&window)? else {
      return Ok(None);
    };
    window.state::<AppState>().pick_import(path).map(Some)
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
        file_name: "aegis-export.json",
      },
      file_types: &[AEGIS_FILE_TYPE],
    };
    let Some(path) = dialog.show(&window)? else {
      return Ok(false);
    };
    state.export_to_aegis(&password, &path)?;
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
    let Some(path) = dialog.show(&window)? else {
      return Ok(None);
    };
    window.state::<AppState>().record_activity();
    let image = read_file(&path, MAX_QR_IMAGE_BYTES)?;
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
