//! The rooted-phone warning, on the app's side: whether to show it, and
//! the user's consent, kept as an empty `root-risk-accepted` file next to
//! the vault (so clearing the app's data asks again).

use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};

use super::AppState;
use crate::{device, dto::DeviceCheck, error::AppError};

impl AppState {
  fn root_consent_path(&self) -> PathBuf {
    self.paths.vault().with_file_name("root-risk-accepted")
  }

  /// Whether the phone looks rooted, and whether the user has accepted
  /// the risk of keeping the vault on it.
  ///
  /// # Errors
  ///
  /// [`AppError::FileRead`] if the consent cannot be looked for.
  pub fn device_check(&self) -> Result<DeviceCheck, AppError> {
    self.check_device(device::rooted())
  }

  fn check_device(&self, rooted: bool) -> Result<DeviceCheck, AppError> {
    let accepted = if rooted {
      let path = self.root_consent_path();
      path
        .try_exists()
        .map_err(|source| AppError::FileRead { path, source })?
    } else {
      false
    };
    Ok(DeviceCheck { rooted, accepted })
  }

  /// Records that the user accepted the risk of a rooted phone, so the
  /// warning is not shown again.
  ///
  /// # Errors
  ///
  /// [`AppError::FileWrite`] if the consent cannot be stored.
  pub fn accept_root_risk(&self) -> Result<(), AppError> {
    let path = self.root_consent_path();
    let write = || {
      if let Some(dir) = path.parent() {
        // As the vault's directory: the app's alone.
        fs::DirBuilder::new()
          .recursive(true)
          .mode(0o700)
          .create(dir)?;
      }
      fs::write(&path, b"")
    };
    write().map_err(|source| AppError::FileWrite {
      path: path.clone(),
      source,
    })
  }
}

#[cfg(test)]
mod tests {
  use crate::state::fixture::*;

  #[test]
  fn consent_is_asked_on_a_rooted_phone_until_given() {
    let fixture = Fixture::new();
    let state = &fixture.state;

    let check = state.check_device(true).unwrap();
    assert!(check.rooted);
    assert!(!check.accepted);

    state.accept_root_risk().unwrap();
    assert!(state.check_device(true).unwrap().accepted);
  }

  #[test]
  fn an_unrooted_phone_needs_no_consent() {
    let fixture = Fixture::new();
    let check = fixture.state.check_device(false).unwrap();
    assert!(!check.rooted);
    assert!(!check.accepted);
  }
}
