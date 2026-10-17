//! Test fixtures: an [`AppState`] over a temporary directory, with the
//! fastest KDF parameters the vault accepts.

use std::fs;

use crypto::KdfParams;
use storage::AppPaths;
use tempfile::TempDir;

use super::AppState;
use crate::dto::SecretString;

pub const PASSWORD: &str = "correct horse battery";

pub fn secret(text: &str) -> SecretString {
  SecretString::from(text.to_owned())
}

pub struct Fixture {
  pub dir: TempDir,
  pub state: AppState,
}

impl Fixture {
  /// No vault yet.
  pub fn new() -> Self {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::with_kdf_params(&Self::paths_in(&dir), KdfParams::MINIMUM);
    Self { dir, state }
  }

  /// A new, empty vault, unlocked with [`PASSWORD`].
  pub fn unlocked() -> Self {
    let fixture = Self::new();
    fixture.state.create_vault(&secret(PASSWORD)).unwrap();
    fixture
  }

  pub fn paths(&self) -> AppPaths {
    Self::paths_in(&self.dir)
  }

  /// Another app instance over the same files.
  pub fn second_instance(&self) -> AppState {
    AppState::with_kdf_params(&self.paths(), KdfParams::MINIMUM)
  }

  /// The same files with a fresh app instance, as after quitting and
  /// relaunching the app.
  pub fn restart(self) -> Self {
    let Self { dir, state } = self;
    drop(state); // Quitting releases the vault's lock.
    let state = AppState::with_kdf_params(&Self::paths_in(&dir), KdfParams::MINIMUM);
    Self { dir, state }
  }

  /// Adds a TOTP account named `label` (all with the same secret).
  pub fn add_account(&self, label: &str) {
    self
      .state
      .add_entry_from_uri(&secret(&format!(
        "otpauth://totp/{label}?secret=JBSWY3DPEHPK3PXP"
      )))
      .unwrap();
  }

  /// The accounts' names, sorted.
  pub fn labels(&self) -> Vec<String> {
    let mut labels: Vec<String> = self
      .state
      .list_entries()
      .unwrap()
      .into_iter()
      .map(|entry| entry.account_label)
      .collect();
    labels.sort();
    labels
  }

  /// How many other devices this vault syncs with.
  pub fn other_devices(&self) -> usize {
    self.state.sync_overview().unwrap().devices.len()
  }

  /// Removes the vault's directory, so that every later save fails.
  pub fn make_vault_unwritable(&self) {
    fs::remove_dir_all(self.paths().vault().parent().unwrap()).unwrap();
  }

  fn paths_in(dir: &TempDir) -> AppPaths {
    AppPaths::in_dirs(
      &dir.path().join("data"),
      &dir.path().join("config"),
      &dir.path().join("state"),
    )
  }
}
