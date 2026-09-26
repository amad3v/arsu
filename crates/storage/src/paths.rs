use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::StorageError;

const VAULT_FILE_NAME: &str = "vault";
const SETTINGS_FILE_NAME: &str = "settings.json";

/// Where the app keeps its files, resolved once from the platform's
/// conventions (the `directories` crate; XDG Base Directory on Linux):
///
/// - vault: `$XDG_DATA_HOME/<application>/vault`, by default
///   `~/.local/share/<application>/vault`;
/// - settings: `$XDG_CONFIG_HOME/<application>/settings.json`, by
///   default `~/.config/<application>/settings.json`.
///
/// The file names are part of the on-disk contract: renaming them (or
/// passing a different `application`) would orphan existing vaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
  vault: PathBuf,
  settings: PathBuf,
}

impl AppPaths {
  /// `qualifier`/`organization`/`application` are passed in rather than
  /// hardcoded — this crate has no opinion on the product's name.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::NoProjectDirs`] if the platform's data and
  /// config directories cannot be determined (e.g. no home directory).
  pub fn resolve(
    qualifier: &str,
    organization: &str,
    application: &str,
  ) -> Result<Self, StorageError> {
    let dirs =
      ProjectDirs::from(qualifier, organization, application).ok_or(StorageError::NoProjectDirs)?;
    Ok(Self::in_dirs(dirs.data_dir(), dirs.config_dir()))
  }

  /// The same layout under explicit directories (tests, portable
  /// installs).
  #[must_use]
  pub fn in_dirs(data_dir: &Path, config_dir: &Path) -> Self {
    Self {
      vault: data_dir.join(VAULT_FILE_NAME),
      settings: config_dir.join(SETTINGS_FILE_NAME),
    }
  }

  #[must_use]
  pub fn vault(&self) -> &Path {
    &self.vault
  }

  #[must_use]
  pub fn settings(&self) -> &Path {
    &self.settings
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keeps_the_established_file_names() {
    let paths = AppPaths::in_dirs(Path::new("/data/app"), Path::new("/config/app"));
    assert_eq!(paths.vault(), Path::new("/data/app/vault"));
    assert_eq!(paths.settings(), Path::new("/config/app/settings.json"));
  }

  #[test]
  fn resolves_both_files_under_the_application_directories() {
    let paths = AppPaths::resolve("com", "example", "arsu-test").unwrap();
    assert!(paths.vault().ends_with("arsu-test/vault"));
    assert!(paths.settings().ends_with("arsu-test/settings.json"));
  }
}
