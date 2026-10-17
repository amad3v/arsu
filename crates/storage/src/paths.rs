use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::StorageError;

const VAULT_FILE_NAME: &str = "vault";
const SETTINGS_FILE_NAME: &str = "settings.json";
const INSTALLATION_ID_FILE_NAME: &str = "installation-id";

/// Where the app keeps its files, resolved once from the platform's
/// conventions (the `directories` crate; XDG Base Directory on Linux):
///
/// - vault: `$XDG_DATA_HOME/<application>/vault`, by default
///   `~/.local/share/<application>/vault`;
/// - settings: `$XDG_CONFIG_HOME/<application>/settings.json`, by
///   default `~/.config/<application>/settings.json`;
/// - installation id (see [`crate::installation_id`]):
///   `$XDG_STATE_HOME/<application>/installation-id`, by default
///   `~/.local/state/<application>/installation-id`.
///
/// The file names are part of the on-disk contract: renaming them (or
/// passing a different `application`) would orphan existing vaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
  vault: PathBuf,
  settings: PathBuf,
  installation_id: PathBuf,
}

impl AppPaths {
  /// `qualifier`/`organization`/`application` are passed in rather than
  /// hardcoded — this crate has no opinion on the product's name.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::NoProjectDirs`] if the platform's data and
  /// config directories cannot be determined (e.g. no home directory).
  /// Where the platform has no state directory, the installation id
  /// goes in the local data directory.
  pub fn resolve(
    qualifier: &str,
    organization: &str,
    application: &str,
  ) -> Result<Self, StorageError> {
    let dirs =
      ProjectDirs::from(qualifier, organization, application).ok_or(StorageError::NoProjectDirs)?;
    let state_dir = dirs.state_dir().unwrap_or_else(|| dirs.data_local_dir());
    Ok(Self::in_dirs(dirs.data_dir(), dirs.config_dir(), state_dir))
  }

  /// The same layout under explicit directories (tests, portable
  /// installs).
  #[must_use]
  pub fn in_dirs(data_dir: &Path, config_dir: &Path, state_dir: &Path) -> Self {
    Self {
      vault: data_dir.join(VAULT_FILE_NAME),
      settings: config_dir.join(SETTINGS_FILE_NAME),
      installation_id: state_dir.join(INSTALLATION_ID_FILE_NAME),
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

  #[must_use]
  pub fn installation_id(&self) -> &Path {
    &self.installation_id
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keeps_the_established_file_names() {
    let paths = AppPaths::in_dirs(
      Path::new("/data/app"),
      Path::new("/config/app"),
      Path::new("/state/app"),
    );
    assert_eq!(paths.vault(), Path::new("/data/app/vault"));
    assert_eq!(paths.settings(), Path::new("/config/app/settings.json"));
    assert_eq!(
      paths.installation_id(),
      Path::new("/state/app/installation-id")
    );
  }

  #[test]
  fn resolves_both_files_under_the_application_directories() {
    let paths = AppPaths::resolve("com", "example", "arsu-test").unwrap();
    assert!(paths.vault().ends_with("arsu-test/vault"));
    assert!(paths.settings().ends_with("arsu-test/settings.json"));
    assert!(
      paths
        .installation_id()
        .ends_with("arsu-test/installation-id")
    );
  }
}
