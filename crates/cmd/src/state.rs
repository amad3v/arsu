//! The state behind the commands: the vault file, the unlocked session,
//! the settings, the file picked for import and the clipboard log.
//!
//! This is plain Rust with no Tauri in it, so it is tested directly; the
//! command layer (`commands.rs`) only moves arguments in and results out.
//!
//! One [`AppState`] is shared by every command. Each part has its own
//! lock, no lock is taken while another is held, and the slow work —
//! Argon2id when creating or unlocking the vault or re-checking the
//! master password, scrypt or PBKDF2 in imports and exports — runs with
//! no lock held, so the rest of the app stays responsive meanwhile.

mod biometric;
mod device;
mod entries;
mod settings;
mod transfer;

#[cfg(test)]
mod fixture;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crypto::KdfParams;
use storage::{AppPaths, SettingsStore, StorageError, UnlockedVault, VaultStorage};
use vault_core::VaultPayload;

use crate::{
  clipboard::CopiedCodes, clock::BootInstant, dto::SecretString, error::AppError, password,
};

use settings::SettingsState;
use transfer::PendingImport;

/// The app's name on disk: the vault lives in `$XDG_DATA_HOME/arsu`
/// and the settings in `$XDG_CONFIG_HOME/arsu`. Part of the on-disk
/// contract — changing it would orphan every existing vault.
#[cfg(not(target_os = "android"))]
const APP_NAME: &str = "arsu";
/// With [`ORGANIZATION`], matches the bundle identifier
/// `io.github.amad3v.arsu`. Neither is part of any path on Linux.
#[cfg(not(target_os = "android"))]
const QUALIFIER: &str = "io.github";
#[cfg(not(target_os = "android"))]
const ORGANIZATION: &str = "amad3v";

/// Where the app keeps its files (see [`AppPaths`]).
///
/// # Errors
///
/// Returns [`StorageError::NoProjectDirs`] if the platform's data and
/// config directories cannot be determined (no home directory).
#[cfg(not(target_os = "android"))]
pub fn app_paths() -> Result<AppPaths, AppError> {
  Ok(AppPaths::resolve(QUALIFIER, ORGANIZATION, APP_NAME)?)
}

/// Where the app keeps its files on Android: in its own private storage,
/// `/data/user/<user>/io.github.amad3v.arsu`, which only it can read,
/// as `vault/vault` and `settings/settings.json`. Each file gets a
/// directory of its own, which the storage crate makes owner-only.
///
/// # Errors
///
/// Returns [`StorageError::NoProjectDirs`] if Android does not give the
/// app's data directory.
#[cfg(target_os = "android")]
pub fn app_paths<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<AppPaths, AppError> {
  use tauri::Manager;

  let root = app
    .path()
    .app_data_dir()
    .map_err(|_| StorageError::NoProjectDirs)?;
  Ok(AppPaths::in_dirs(
    &root.join(APP_DIRS.0),
    &root.join(APP_DIRS.1),
  ))
}

/// The directories of the vault and the settings on Android, under the
/// app's data directory. Part of the on-disk contract there, as the
/// app's name is on Linux.
#[cfg(target_os = "android")]
const APP_DIRS: (&str, &str) = ("vault", "settings");

/// Everything the commands share: one instance, managed by Tauri.
pub struct AppState {
  paths: AppPaths,
  kdf_params: KdfParams,
  /// Opened on first use, which also takes the lock that keeps a second
  /// instance of the app off the vault; see [`Self::storage`].
  storage: Mutex<Option<Arc<VaultStorage>>>,
  session: Mutex<Session>,
  settings: Mutex<SettingsState>,
  pending_import: Mutex<Option<PendingImport>>,
  copied_codes: CopiedCodes,
}

impl AppState {
  /// The state for the vault and settings at `paths`. Reads the settings
  /// now; the vault file is opened when a command first needs it.
  #[must_use]
  pub fn new(paths: &AppPaths) -> Self {
    Self::with_kdf_params(paths, KdfParams::RECOMMENDED)
  }

  /// [`Self::new`] with the KDF parameters for new vaults, and for
  /// upgrading weaker ones on unlock.
  fn with_kdf_params(paths: &AppPaths, kdf_params: KdfParams) -> Self {
    Self {
      paths: paths.clone(),
      kdf_params,
      storage: Mutex::new(None),
      session: Mutex::new(Session {
        vault: None,
        last_activity: BootInstant::now(),
      }),
      settings: Mutex::new(SettingsState::load(SettingsStore::new(
        paths.settings().to_path_buf(),
      ))),
      pending_import: Mutex::new(None),
      copied_codes: CopiedCodes::default(),
    }
  }

  /// Where the vault and settings files are.
  #[must_use]
  pub fn paths(&self) -> &AppPaths {
    &self.paths
  }

  /// The vault file's handle, opening it on first use.
  ///
  /// Opening is lazy so that a vault held by another instance of the app
  /// is reported to the frontend as [`StorageError::InUse`] by the first
  /// command, rather than aborting startup — and so that a later command
  /// can succeed once that instance has quit.
  fn storage(&self) -> Result<Arc<VaultStorage>, AppError> {
    let mut storage = lock(&self.storage);
    if let Some(opened) = &*storage {
      return Ok(Arc::clone(opened));
    }
    let opened = Arc::new(
      VaultStorage::new(self.paths.vault().to_path_buf())?.with_kdf_params(self.kdf_params),
    );
    *storage = Some(Arc::clone(&opened));
    Ok(opened)
  }

  fn session(&self) -> MutexGuard<'_, Session> {
    lock(&self.session)
  }

  /// # Errors
  ///
  /// Returns [`StorageError::InUse`] if another instance of the app has
  /// the vault open, or [`StorageError::Io`] if the vault directory
  /// cannot be accessed.
  pub fn vault_exists(&self) -> Result<bool, AppError> {
    Ok(self.storage()?.exists()?)
  }

  /// Creates the vault, encrypted under `master_password`, and unlocks
  /// it.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::WeakPassword`] if the password is too short,
  /// [`StorageError::AlreadyExists`] if there is a vault already, or an
  /// error if the vault cannot be written.
  pub fn create_vault(&self, master_password: &SecretString) -> Result<(), AppError> {
    password::ensure_strong(master_password)?;
    let vault = self
      .storage()?
      .create(master_password.expose().as_bytes())?;
    self.start_session(vault);
    // A key sealed for a vault that went missing cannot open this one.
    self.remove_biometric_slot()?;
    Ok(())
  }

  /// # Errors
  ///
  /// Returns [`StorageError::NotFound`] if there is no vault, a
  /// [`crypto::CryptoError::Decrypt`] error for a wrong password (or a
  /// damaged file — the two are indistinguishable by design), or an
  /// error if the file is not a vault this version can read.
  pub fn unlock_vault(&self, master_password: &SecretString) -> Result<(), AppError> {
    let vault = self.storage()?.open(master_password.expose().as_bytes())?;
    self.start_session(vault);
    Ok(())
  }

  fn start_session(&self, vault: UnlockedVault) {
    let mut session = self.session();
    session.vault = Some(vault);
    session.record_activity();
  }

  /// Drops the key and every decrypted seed from memory.
  pub fn lock_vault(&self) {
    self.session().vault = None;
  }

  /// Locks the vault if it is unlocked; tells whether it was.
  pub(crate) fn lock_if_unlocked(&self) -> bool {
    self.session().vault.take().is_some()
  }

  #[must_use]
  pub fn is_unlocked(&self) -> bool {
    self.session().vault.is_some()
  }

  /// Notes that the user just did something, postponing the auto-lock.
  pub fn record_activity(&self) {
    self.session().record_activity();
  }

  /// Locks the vault if it has been idle for the auto-lock time as of
  /// `now`. Returns whether it did.
  pub(crate) fn lock_if_idle(&self, now: BootInstant) -> bool {
    let idle_limit = self.auto_lock_after();
    let mut session = self.session();
    let idle =
      session.vault.is_some() && now.saturating_duration_since(session.last_activity) >= idle_limit;
    if idle {
      session.vault = None;
    }
    idle
  }

  #[must_use]
  pub fn copied_codes(&self) -> &CopiedCodes {
    &self.copied_codes
  }
}

/// The unlocked vault, if any, and when the user last did something.
struct Session {
  vault: Option<UnlockedVault>,
  last_activity: BootInstant,
}

/// How a [`Session::commit`] writes the vault: [`VaultStorage::save`]
/// for edits worth a backup, [`VaultStorage::save_without_rotation`] for
/// bookkeeping such as HOTP counters.
type Save = fn(&VaultStorage, &UnlockedVault) -> Result<(), StorageError>;

impl Session {
  fn unlocked(&self) -> Result<&UnlockedVault, AppError> {
    self.vault.as_ref().ok_or(AppError::Locked)
  }

  fn record_activity(&mut self) {
    self.last_activity = BootInstant::now();
  }

  /// Applies `change` to the vault's contents and saves them with
  /// `save` — all or nothing: if `change` or the save fails, the
  /// contents in memory are restored, so the app never shows a change
  /// that did not reach the disk.
  fn commit<T>(
    &mut self,
    storage: &VaultStorage,
    save: Save,
    change: impl FnOnce(&mut VaultPayload) -> Result<T, AppError>,
  ) -> Result<T, AppError> {
    let vault = self.vault.as_mut().ok_or(AppError::Locked)?;
    let before = vault.payload.clone();
    let result = change(&mut vault.payload)
      .and_then(|value| save(storage, vault).map(|()| value).map_err(AppError::from));
    if result.is_err() {
      vault.payload = before;
    }
    result
  }
}

/// Locks `mutex`. Every critical section in this module either
/// completes or leaves the data as it found it, so a lock poisoned by a
/// panic elsewhere is safe to keep using (and release builds abort on
/// panic anyway).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::error::AppErrorKind;

  use super::{fixture::*, *};

  #[cfg(not(target_os = "android"))]
  #[test]
  fn the_data_directory_keeps_its_name() {
    let paths = app_paths().unwrap();
    assert!(paths.vault().ends_with("arsu/vault"), "{paths:?}");
    assert!(
      paths.settings().ends_with("arsu/settings.json"),
      "{paths:?}"
    );
  }

  #[test]
  fn create_lock_unlock_round_trip() {
    let fixture = Fixture::new();
    let state = &fixture.state;
    assert!(!state.vault_exists().unwrap());
    assert!(!state.is_unlocked());

    state.create_vault(&secret(PASSWORD)).unwrap();
    assert!(state.vault_exists().unwrap());
    assert!(state.is_unlocked());

    state.lock_vault();
    assert!(!state.is_unlocked());
    assert_eq!(
      state.list_entries().unwrap_err().kind(),
      AppErrorKind::Locked
    );

    state.unlock_vault(&secret(PASSWORD)).unwrap();
    assert!(state.is_unlocked());
  }

  #[test]
  fn creating_needs_a_strong_password_and_no_vault() {
    let fixture = Fixture::new();
    let state = &fixture.state;

    let weak = state.create_vault(&secret("short")).unwrap_err();
    assert_eq!(weak.kind(), AppErrorKind::WeakPassword);
    assert!(!state.vault_exists().unwrap());

    state.create_vault(&secret(PASSWORD)).unwrap();
    let again = state.create_vault(&secret(PASSWORD)).unwrap_err();
    assert_eq!(again.kind(), AppErrorKind::VaultAlreadyExists);
  }

  #[test]
  fn unlocking_reports_a_missing_vault_and_a_wrong_password() {
    let fixture = Fixture::new();
    let state = &fixture.state;
    assert_eq!(
      state.unlock_vault(&secret(PASSWORD)).unwrap_err().kind(),
      AppErrorKind::VaultNotFound
    );

    state.create_vault(&secret(PASSWORD)).unwrap();
    state.lock_vault();
    assert_eq!(
      state
        .unlock_vault(&secret("not the password"))
        .unwrap_err()
        .kind(),
      AppErrorKind::WrongPassword
    );
    assert!(!state.is_unlocked());
  }

  #[test]
  fn a_second_instance_is_told_the_vault_is_in_use() {
    let fixture = Fixture::new();
    fixture.state.create_vault(&secret(PASSWORD)).unwrap();

    let second = fixture.second_instance();
    assert_eq!(
      second.vault_exists().unwrap_err().kind(),
      AppErrorKind::VaultInUse
    );

    drop(fixture.state);
    assert!(
      second.vault_exists().unwrap(),
      "usable once the first quits"
    );
  }

  #[test]
  fn locking_when_unlocked_says_whether_it_was() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    assert!(state.lock_if_unlocked());
    assert!(!state.is_unlocked());
    assert!(!state.lock_if_unlocked(), "already locked: nothing to tell");
  }

  #[test]
  fn locks_after_the_idle_time_and_not_before() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let limit = Duration::from_mins(5);
    let unlocked_at = BootInstant::now();

    assert!(!state.lock_if_idle(unlocked_at + Duration::from_secs(290)));
    assert!(state.is_unlocked());

    assert!(state.lock_if_idle(unlocked_at + limit + Duration::from_secs(1)));
    assert!(!state.is_unlocked());
    assert!(
      !state.lock_if_idle(unlocked_at + limit * 2),
      "already locked"
    );
  }

  #[test]
  fn activity_postpones_the_auto_lock() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let limit = Duration::from_mins(5);
    let start = BootInstant::now();
    // The user does something four minutes in.
    let activity = start + Duration::from_mins(4);
    state.session().last_activity = activity;

    assert!(!state.lock_if_idle(start + limit + Duration::from_secs(1)));
    assert!(state.lock_if_idle(activity + limit));
  }

  #[test]
  fn user_actions_count_as_activity_but_code_refreshes_do_not() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state
      .add_entry_from_uri(&secret(
        "otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP",
      ))
      .unwrap();
    let long_ago = BootInstant::now();
    state.session().last_activity = long_ago;

    state.current_code(id, 59).unwrap();
    state.list_entries().unwrap();
    assert_eq!(state.session().last_activity, long_ago);

    state.record_activity();
    assert!(state.session().last_activity > long_ago);
  }

  #[test]
  fn a_failed_save_leaves_the_entries_as_they_were() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let kept = state
      .add_entry_from_uri(&secret(
        "otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP",
      ))
      .unwrap();

    fixture.make_vault_unwritable();

    let add = state.add_entry_from_uri(&secret("otpauth://totp/Other:bob?secret=JBSWY3DPEHPK3PXP"));
    assert_eq!(add.unwrap_err().kind(), AppErrorKind::StorageIo);
    let delete = state.delete_entry(kept);
    assert_eq!(delete.unwrap_err().kind(), AppErrorKind::StorageIo);

    let entries = state.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, kept.to_string());
  }
}
