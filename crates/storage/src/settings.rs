//! User preferences, stored as JSON in the platform config directory
//! (see [`crate::AppPaths`]).
//!
//! Every field is validated on the way in — a hand-edited or
//! frontend-supplied value outside what the app supports is rejected
//! instead of silently persisted — and missing fields fall back to
//! their defaults, so settings files written by earlier versions (which
//! only had `theme`) keep loading.

use std::{
  fs, io,
  ops::RangeInclusive,
  path::{Path, PathBuf},
  time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{StorageError, fs_util};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
  #[default]
  System,
  Light,
  Dark,
}

/// How long the vault may sit idle before it locks itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct AutoLockMinutes(u32);

impl AutoLockMinutes {
  /// The choices the app offers; anything else is rejected.
  pub const ALLOWED: [u32; 6] = [1, 2, 5, 10, 15, 30];
  pub const DEFAULT: Self = Self(5);

  #[must_use]
  pub const fn get(self) -> u32 {
    self.0
  }

  #[must_use]
  pub fn duration(self) -> Duration {
    Duration::from_mins(u64::from(self.0))
  }
}

impl Default for AutoLockMinutes {
  fn default() -> Self {
    Self::DEFAULT
  }
}

impl TryFrom<u32> for AutoLockMinutes {
  type Error = InvalidSetting;

  fn try_from(minutes: u32) -> Result<Self, Self::Error> {
    if Self::ALLOWED.contains(&minutes) {
      Ok(Self(minutes))
    } else {
      Err(InvalidSetting::AutoLockMinutes(minutes))
    }
  }
}

impl From<AutoLockMinutes> for u32 {
  fn from(minutes: AutoLockMinutes) -> Self {
    minutes.0
  }
}

/// How long a copied code stays on the clipboard before it is cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct ClipboardClearSeconds(u32);

impl ClipboardClearSeconds {
  pub const RANGE: RangeInclusive<u32> = 10..=60;
  pub const DEFAULT: Self = Self(20);

  #[must_use]
  pub const fn get(self) -> u32 {
    self.0
  }

  #[must_use]
  pub fn duration(self) -> Duration {
    Duration::from_secs(u64::from(self.0))
  }
}

impl Default for ClipboardClearSeconds {
  fn default() -> Self {
    Self::DEFAULT
  }
}

impl TryFrom<u32> for ClipboardClearSeconds {
  type Error = InvalidSetting;

  fn try_from(seconds: u32) -> Result<Self, Self::Error> {
    if Self::RANGE.contains(&seconds) {
      Ok(Self(seconds))
    } else {
      Err(InvalidSetting::ClipboardClearSeconds(seconds))
    }
  }
}

impl From<ClipboardClearSeconds> for u32 {
  fn from(seconds: ClipboardClearSeconds) -> Self {
    seconds.0
  }
}

/// A setting value outside what the app supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidSetting {
  #[error(
    "auto-lock must be one of {allowed:?} minutes, got {0}",
    allowed = AutoLockMinutes::ALLOWED
  )]
  AutoLockMinutes(u32),
  #[error(
    "clipboard clearing must be between {min} and {max} seconds, got {0}",
    min = ClipboardClearSeconds::RANGE.start(),
    max = ClipboardClearSeconds::RANGE.end()
  )]
  ClipboardClearSeconds(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
  pub theme: Theme,
  pub auto_lock_minutes: AutoLockMinutes,
  pub clipboard_clear_seconds: ClipboardClearSeconds,
}

/// Reads and writes the settings file.
#[derive(Debug, Clone)]
pub struct SettingsStore {
  path: PathBuf,
}

impl SettingsStore {
  #[must_use]
  pub fn new(path: PathBuf) -> Self {
    Self { path }
  }

  #[must_use]
  pub fn path(&self) -> &Path {
    &self.path
  }

  /// Read the settings. A missing file means "never changed", so it
  /// yields the defaults — without creating the file; only
  /// [`Self::save`] writes.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::Io`] if the file exists but cannot be read,
  /// or [`StorageError::Settings`] if it is not valid settings JSON
  /// (including a value outside what the app supports).
  pub fn load(&self) -> Result<Settings, StorageError> {
    match fs::read(&self.path) {
      Ok(json) => serde_json::from_slice(&json).map_err(|source| StorageError::Settings {
        path: self.path.clone(),
        source,
      }),
      Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
      Err(e) => Err(StorageError::io(&self.path)(e)),
    }
  }

  /// Write the settings crash-safely (temp file, fsync, rename),
  /// creating the config directory if needed.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::Io`] if the directory or file cannot be
  /// written, or [`StorageError::Settings`] if serialization fails.
  pub fn save(&self, settings: &Settings) -> Result<(), StorageError> {
    let json = serde_json::to_vec_pretty(settings).map_err(|source| StorageError::Settings {
      path: self.path.clone(),
      source,
    })?;
    let dir = fs_util::parent_dir(&self.path).map_err(StorageError::io(&self.path))?;
    fs::create_dir_all(dir).map_err(StorageError::io(dir))?;
    fs_util::atomic_write(&self.path, &json).map_err(StorageError::io(&self.path))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn store_in(dir: &tempfile::TempDir) -> SettingsStore {
    SettingsStore::new(dir.path().join("arsu").join("settings.json"))
  }

  #[test]
  fn missing_file_loads_defaults_without_creating_anything() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);

    assert_eq!(store.load().unwrap(), Settings::default());
    assert!(!store.path().exists(), "load must not write");
  }

  #[test]
  fn defaults_are_the_documented_ones() {
    let settings = Settings::default();
    assert_eq!(settings.theme, Theme::System);
    assert_eq!(settings.auto_lock_minutes.get(), 5);
    assert_eq!(settings.clipboard_clear_seconds.get(), 20);
  }

  #[test]
  fn save_then_load_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let settings = Settings {
      theme: Theme::Dark,
      auto_lock_minutes: AutoLockMinutes::try_from(15).unwrap(),
      clipboard_clear_seconds: ClipboardClearSeconds::try_from(45).unwrap(),
    };

    store.save(&settings).unwrap();

    assert_eq!(store.load().unwrap(), settings);
    let siblings = fs::read_dir(store.path().parent().unwrap())
      .unwrap()
      .count();
    assert_eq!(siblings, 1, "no temp file left behind");
  }

  #[test]
  fn file_format_is_snake_case_json_with_lowercase_theme() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    store.save(&Settings::default()).unwrap();

    let json: serde_json::Value = serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
    assert_eq!(
      json,
      serde_json::json!({
        "theme": "system",
        "auto_lock_minutes": 5,
        "clipboard_clear_seconds": 20,
      })
    );
  }

  #[test]
  fn a_file_from_an_earlier_version_loads_with_defaults_for_new_fields() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    fs::write(store.path(), br#"{ "theme": "dark" }"#).unwrap();

    assert_eq!(
      store.load().unwrap(),
      Settings {
        theme: Theme::Dark,
        ..Settings::default()
      }
    );
  }

  #[test]
  fn invalid_values_are_rejected_not_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();

    for json in [
      r#"{ "theme": "blue" }"#,
      r#"{ "auto_lock_minutes": 7 }"#,
      r#"{ "clipboard_clear_seconds": 5 }"#,
      r#"{ "clipboard_clear_seconds": 61 }"#,
      "not json",
    ] {
      fs::write(store.path(), json).unwrap();
      assert!(
        matches!(store.load(), Err(StorageError::Settings { .. })),
        "{json}"
      );
    }
  }

  #[test]
  fn setting_bounds() {
    for minutes in AutoLockMinutes::ALLOWED {
      assert_eq!(AutoLockMinutes::try_from(minutes).unwrap().get(), minutes);
    }
    assert_eq!(
      AutoLockMinutes::try_from(0),
      Err(InvalidSetting::AutoLockMinutes(0))
    );
    assert_eq!(AutoLockMinutes::DEFAULT.duration(), Duration::from_mins(5));

    assert!(ClipboardClearSeconds::try_from(10).is_ok());
    assert!(ClipboardClearSeconds::try_from(60).is_ok());
    assert_eq!(
      ClipboardClearSeconds::try_from(9),
      Err(InvalidSetting::ClipboardClearSeconds(9))
    );
    assert_eq!(
      InvalidSetting::ClipboardClearSeconds(9).to_string(),
      "clipboard clearing must be between 10 and 60 seconds, got 9"
    );
    assert_eq!(
      InvalidSetting::AutoLockMinutes(7).to_string(),
      "auto-lock must be one of [1, 2, 5, 10, 15, 30] minutes, got 7"
    );
  }
}
