//! Crash-safe file replacement, shared by the vault and the settings
//! file.
//!
//! New contents are written to a temporary file *in the target's own
//! directory* (a `rename` across filesystems is neither atomic nor
//! guaranteed to work), flushed with `fsync`, and only then renamed
//! over the target — `rename(2)` replaces it atomically, so a reader
//! sees either the complete old file or the complete new one. The
//! directory is fsynced afterwards so the rename itself survives a
//! power loss. Temporary files are owner-only from the moment they are
//! created, never chmod'd afterwards.

use std::{
  ffi::OsString,
  fs::{self, DirBuilder, File, Permissions},
  io::{self, Write},
  os::unix::fs::{DirBuilderExt, PermissionsExt},
  path::{Path, PathBuf},
};

use tempfile::{Builder, NamedTempFile};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIR_MODE: u32 = 0o700;

/// New contents for a file, complete and durable in a temporary file
/// next to it but not yet visible at the target path.
pub(crate) struct StagedFile {
  temp: NamedTempFile,
  target: PathBuf,
}

impl StagedFile {
  /// Atomically replace the target with the staged contents.
  pub(crate) fn commit(self) -> io::Result<()> {
    self.temp.persist(&self.target).map_err(|e| e.error)?;
    sync_parent(&self.target)
  }

  /// Like [`Self::commit`], but fails with
  /// [`io::ErrorKind::AlreadyExists`] instead of replacing an existing
  /// target — atomically, so there is no check-then-write window.
  pub(crate) fn commit_new(self) -> io::Result<()> {
    self
      .temp
      .persist_noclobber(&self.target)
      .map_err(|e| e.error)?;
    sync_parent(&self.target)
  }
}

/// Write `bytes` to a temporary file next to `target` and fsync it.
/// Dropping the returned [`StagedFile`] without committing deletes it.
pub(crate) fn stage(target: &Path, bytes: &[u8]) -> io::Result<StagedFile> {
  let mut temp = Builder::new()
    .prefix(&temp_prefix(target)?)
    .permissions(Permissions::from_mode(PRIVATE_FILE_MODE))
    .tempfile_in(parent_dir(target)?)?;
  temp.write_all(bytes)?;
  temp.as_file().sync_all()?;
  Ok(StagedFile {
    temp,
    target: target.to_path_buf(),
  })
}

/// Replace `target` with `bytes`, crash-safely, as a file only its owner
/// can read or write (mode `0600`) — whatever the process umask, and
/// whatever the mode of a file previously at `target`.
///
/// The directory must already exist. Besides the vault and the settings,
/// this is how the app writes any file that holds secrets, such as an
/// encrypted export of the vault.
///
/// # Errors
///
/// Returns the [`io::Error`] of the first step that fails; `target` is
/// then untouched.
pub fn atomic_write(target: &Path, bytes: &[u8]) -> io::Result<()> {
  stage(target, bytes)?.commit()
}

/// Make the latest renames, links and removals in `path`'s directory
/// durable.
pub(crate) fn sync_parent(path: &Path) -> io::Result<()> {
  File::open(parent_dir(path)?)?.sync_all()
}

/// The directory `path` lives in.
pub(crate) fn parent_dir(path: &Path) -> io::Result<&Path> {
  path
    .parent()
    .filter(|dir| !dir.as_os_str().is_empty())
    .ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        "path does not name a file inside a directory",
      )
    })
}

/// Create `dir` (and missing ancestors, per the XDG spec) as `0700`,
/// and remove group/other access from it if it already exists.
pub(crate) fn ensure_private_dir(dir: &Path) -> io::Result<()> {
  DirBuilder::new()
    .recursive(true)
    .mode(PRIVATE_DIR_MODE)
    .create(dir)?;
  let mode = fs::metadata(dir)?.permissions().mode();
  if mode & 0o077 != 0 {
    fs::set_permissions(dir, Permissions::from_mode(mode & !0o077))?;
  }
  Ok(())
}

/// Delete temporary files left next to `target` by writes that never
/// committed (a crash or a kill mid-write). Only safe while no other
/// writer can be staging a file for `target`.
pub(crate) fn remove_stale_temp_files(target: &Path) -> io::Result<()> {
  let prefix = temp_prefix(target)?;
  for entry in fs::read_dir(parent_dir(target)?)? {
    let entry = entry?;
    let is_stale_temp = entry
      .file_name()
      .as_encoded_bytes()
      .starts_with(prefix.as_encoded_bytes());
    if is_stale_temp && entry.file_type()?.is_file() {
      remove_if_exists(&entry.path())?;
    }
  }
  Ok(())
}

/// `fs::remove_file`, treating "already gone" as success.
pub(crate) fn remove_if_exists(path: &Path) -> io::Result<()> {
  match fs::remove_file(path) {
    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
    result => result,
  }
}

/// `.<file name>-`: hidden, and specific to one target so stale-file
/// cleanup never touches another file's temporaries. For the vault this
/// is `.vault-`, the prefix every earlier version used too.
fn temp_prefix(target: &Path) -> io::Result<OsString> {
  let name = target.file_name().ok_or_else(|| {
    io::Error::new(
      io::ErrorKind::InvalidInput,
      "path does not name a file inside a directory",
    )
  })?;
  let mut prefix = OsString::from(".");
  prefix.push(name);
  prefix.push("-");
  Ok(prefix)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn atomic_write_replaces_the_target_with_owner_only_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("settings.json");
    fs::write(&target, b"old").unwrap();

    atomic_write(&target, b"new").unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(
      fs::metadata(&target).unwrap().permissions().mode() & 0o777,
      PRIVATE_FILE_MODE
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no temp left");
  }

  #[test]
  fn an_uncommitted_stage_leaves_the_target_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("vault");
    fs::write(&target, b"old").unwrap();

    drop(stage(&target, b"new").unwrap());

    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "temp deleted");
  }

  #[test]
  fn commit_new_never_replaces_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("vault");
    fs::write(&target, b"first").unwrap();

    let err = stage(&target, b"second").unwrap().commit_new().unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&target).unwrap(), b"first");
  }

  #[test]
  fn private_dir_is_created_0700_and_tightened_if_looser() {
    let root = tempfile::tempdir().unwrap();
    let created = root.path().join("a/b");
    ensure_private_dir(&created).unwrap();
    assert_eq!(
      fs::metadata(&created).unwrap().permissions().mode() & 0o777,
      PRIVATE_DIR_MODE
    );

    let loose = root.path().join("loose");
    fs::create_dir(&loose).unwrap();
    fs::set_permissions(&loose, Permissions::from_mode(0o755)).unwrap();
    ensure_private_dir(&loose).unwrap();
    assert_eq!(
      fs::metadata(&loose).unwrap().permissions().mode() & 0o777,
      PRIVATE_DIR_MODE
    );
  }

  #[test]
  fn stale_temp_cleanup_only_removes_this_targets_temporaries() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("vault");
    for name in [
      ".vault-abc123",
      ".vault-XYZ",
      ".settings.json-abc123",
      "vault",
      "vault.bak.1",
    ] {
      fs::write(dir.path().join(name), b"x").unwrap();
    }

    remove_stale_temp_files(&target).unwrap();

    let mut left: Vec<_> = fs::read_dir(dir.path())
      .unwrap()
      .map(|e| e.unwrap().file_name().into_string().unwrap())
      .collect();
    left.sort();
    assert_eq!(left, [".settings.json-abc123", "vault", "vault.bak.1"]);
  }

  #[test]
  fn paths_without_a_directory_are_rejected() {
    for path in ["/", "vault", ""] {
      assert_eq!(
        stage(Path::new(path), b"x").err().map(|e| e.kind()),
        Some(io::ErrorKind::InvalidInput),
        "{path:?}"
      );
    }
  }
}
