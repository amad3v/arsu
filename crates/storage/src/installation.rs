//! The installation id: which copy of the app this is.
//!
//! A random id kept in a small file outside the vault. The vault's sync
//! identity records the id of the installation that created it, so a
//! vault file copied to another machine is recognised there and gets an
//! identity of its own instead of sharing the original device's key.
//!
//! On Linux the file lives in `$XDG_STATE_HOME`, the XDG directory for
//! state that is "not important or portable enough" to move with the
//! user's files. The id is not secret.

use std::{fs, io, path::Path};

use uuid::Uuid;

use crate::{
  StorageError,
  fs_util::{atomic_write, ensure_private_dir, parent_dir},
};

/// This installation's id, read from `path`, or created there on first
/// use. A file that does not hold an id (damaged, edited) is replaced by
/// a new one: the installation is then treated as new, and its device
/// pairs again — the safe outcome.
///
/// # Errors
///
/// [`StorageError::Io`] if the file cannot be read or written.
pub fn installation_id(path: &Path) -> Result<Uuid, StorageError> {
  match fs::read_to_string(path) {
    Ok(text) => {
      if let Ok(id) = Uuid::parse_str(text.trim()) {
        return Ok(id);
      }
    }
    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
    Err(error) => return Err(StorageError::io(path)(error)),
  }
  let dir = parent_dir(path).map_err(StorageError::io(path))?;
  ensure_private_dir(dir).map_err(StorageError::io(dir))?;
  let id = Uuid::now_v7();
  atomic_write(path, format!("{id}\n").as_bytes()).map_err(StorageError::io(path))?;
  Ok(id)
}

#[cfg(test)]
mod tests {
  use std::os::unix::fs::PermissionsExt;

  use super::*;

  #[test]
  fn the_first_call_creates_an_id_that_later_calls_return() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state/arsu/installation-id");

    let first = installation_id(&path).unwrap();

    assert!(path.exists());
    assert_eq!(installation_id(&path).unwrap(), first);
  }

  #[test]
  fn different_installations_get_different_ids() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());

    assert_ne!(
      installation_id(&a.path().join("id")).unwrap(),
      installation_id(&b.path().join("id")).unwrap()
    );
  }

  #[test]
  fn a_damaged_file_is_replaced_by_a_new_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("installation-id");
    fs::write(&path, "not an id").unwrap();

    let id = installation_id(&path).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap().trim(), id.to_string());
  }

  #[test]
  fn its_directory_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state/installation-id");

    installation_id(&path).unwrap();

    let mode = fs::metadata(path.parent().unwrap())
      .unwrap()
      .permissions()
      .mode();
    assert_eq!(mode & 0o077, 0);
  }

  #[test]
  fn an_unreadable_location_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    fs::write(&blocker, "").unwrap();

    assert!(matches!(
      installation_id(&blocker.join("installation-id")),
      Err(StorageError::Io { .. })
    ));
  }
}
