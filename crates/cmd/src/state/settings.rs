//! The settings in effect, and changing them.

use std::time::Duration;

use storage::SettingsStore;

use super::{AppState, lock};
use crate::{
  dto::{Settings, SettingsUpdate},
  error::AppError,
};

pub(super) struct SettingsState {
  store: SettingsStore,
  in_effect: storage::Settings,
  /// The settings file could not be read, so the defaults are in effect.
  /// [`AppState::settings`] reports it (and tries again) until a read or
  /// a save succeeds.
  unreadable: bool,
}

impl SettingsState {
  pub(super) fn load(store: SettingsStore) -> Self {
    // The error itself is not lost: `AppState::settings` reads the file
    // again and reports it to the frontend.
    let (in_effect, unreadable) = store.load().map_or_else(
      |_| (storage::Settings::default(), true),
      |loaded| (loaded, false),
    );
    Self {
      store,
      in_effect,
      unreadable,
    }
  }
}

impl AppState {
  /// The settings in effect.
  ///
  /// # Errors
  ///
  /// Returns [`storage::StorageError::Settings`] (kind
  /// `InvalidSettingsFile`) or [`storage::StorageError::Io`] while the
  /// settings file cannot be read. The defaults are in effect meanwhile,
  /// and the next [`Self::update_settings`] replaces the file.
  pub fn settings(&self) -> Result<Settings, AppError> {
    let mut settings = lock(&self.settings);
    if settings.unreadable {
      settings.in_effect = settings.store.load()?;
      settings.unreadable = false;
    }
    Ok(Settings::from(&settings.in_effect))
  }

  /// Applies `update`, saves the result and returns it. Nothing changes,
  /// in memory or on disk, unless the save succeeds.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::InvalidSetting`] for a value the app does not
  /// support, or a [`storage::StorageError`] if the file cannot be
  /// written.
  pub fn update_settings(&self, update: SettingsUpdate) -> Result<Settings, AppError> {
    self.record_activity();
    let mut settings = lock(&self.settings);
    let updated = update.apply_to(&settings.in_effect)?;
    settings.store.save(&updated)?;
    settings.in_effect = updated;
    settings.unreadable = false;
    Ok(Settings::from(&settings.in_effect))
  }

  pub(super) fn auto_lock_after(&self) -> Duration {
    lock(&self.settings).in_effect.auto_lock_minutes.duration()
  }

  /// How long a copied code stays on the clipboard.
  #[must_use]
  pub fn clipboard_clear_after(&self) -> Duration {
    lock(&self.settings)
      .in_effect
      .clipboard_clear_seconds
      .duration()
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use storage::Theme;

  use super::*;
  use crate::{clock::BootInstant, error::AppErrorKind, state::fixture::*};

  fn update(json: serde_json::Value) -> SettingsUpdate {
    serde_json::from_value(json).unwrap()
  }

  #[test]
  fn defaults_apply_until_changed_and_changes_persist() {
    let fixture = Fixture::new();
    assert_eq!(
      fixture.state.settings().unwrap(),
      Settings {
        theme: Theme::System,
        auto_lock_minutes: 5,
        clipboard_clear_seconds: 20,
      }
    );

    let updated = fixture
      .state
      .update_settings(update(
        serde_json::json!({ "theme": "dark", "clipboardClearSeconds": 45 }),
      ))
      .unwrap();
    assert_eq!(updated.theme, Theme::Dark);
    assert_eq!(
      fixture.state.clipboard_clear_after(),
      Duration::from_secs(45)
    );

    let restarted = fixture.restart();
    assert_eq!(restarted.state.settings().unwrap(), updated);
  }

  #[test]
  fn the_auto_lock_time_comes_from_the_settings() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    state
      .update_settings(update(serde_json::json!({ "autoLockMinutes": 1 })))
      .unwrap();

    assert!(state.lock_if_idle(BootInstant::now() + Duration::from_secs(61)));
  }

  #[test]
  fn unsupported_values_change_nothing() {
    let fixture = Fixture::new();
    let error = fixture
      .state
      .update_settings(update(
        serde_json::json!({ "theme": "light", "autoLockMinutes": 7 }),
      ))
      .unwrap_err();

    assert_eq!(error.kind(), AppErrorKind::InvalidAutoLockMinutes);
    assert_eq!(fixture.state.settings().unwrap().theme, Theme::System);
    assert!(!fixture.paths().settings().exists());
  }

  #[test]
  fn a_failed_save_changes_nothing_in_memory() {
    let fixture = Fixture::new();
    // A file where the config directory should be makes every save fail.
    fs::write(fixture.paths().settings().parent().unwrap(), b"").unwrap();

    let error = fixture
      .state
      .update_settings(update(serde_json::json!({ "theme": "dark" })))
      .unwrap_err();

    assert_eq!(error.kind(), AppErrorKind::StorageIo);
    assert_eq!(fixture.state.settings().unwrap().theme, Theme::System);
  }

  #[test]
  fn an_invalid_settings_file_is_reported_not_silently_replaced() {
    let fixture = Fixture::new();
    let settings_path = fixture.paths().settings().to_path_buf();
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(&settings_path, br#"{ "theme": "drak" }"#).unwrap();

    let restarted = fixture.restart();
    let state = &restarted.state;
    assert_eq!(
      state.settings().unwrap_err().kind(),
      AppErrorKind::InvalidSettingsFile
    );
    assert_eq!(
      state.auto_lock_after(),
      Duration::from_mins(5),
      "defaults apply"
    );
    assert_eq!(fs::read(&settings_path).unwrap(), br#"{ "theme": "drak" }"#);

    let updated = state
      .update_settings(update(serde_json::json!({ "theme": "light" })))
      .unwrap();
    assert_eq!(updated.theme, Theme::Light);
    assert_eq!(state.settings().unwrap(), updated);
  }
}
