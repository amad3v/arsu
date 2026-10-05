//! Reading and writing the files the user picks in a file dialog.

use std::{
  fs::File,
  io::Read,
  path::{Path, PathBuf},
};

use interop::InteropError;
use rustix::fs::{Mode, OFlags};
use zeroize::Zeroizing;

use crate::error::AppError;

/// A file the user chose in a file dialog ([`crate::file_dialog`]).
pub trait UserFile {
  /// The file's name, as the user sees it.
  fn name(&self) -> Option<String>;

  /// Where the file is, for error messages.
  fn location(&self) -> PathBuf;

  /// Reads the file, refusing it if it is larger than `max_bytes`. The
  /// bytes may be an unencrypted backup, so they are zeroized when
  /// dropped.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::FileRead`], [`AppError::NotAFile`], or
  /// `FileTooLarge`.
  fn read(&self, max_bytes: u64) -> Result<Zeroizing<Vec<u8>>, AppError>;

  /// Replaces the file's contents with `bytes`.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::FileWrite`].
  fn write(&self, bytes: &[u8]) -> Result<(), AppError>;
}

impl UserFile for PathBuf {
  fn name(&self) -> Option<String> {
    self
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
  }

  fn location(&self) -> PathBuf {
    self.clone()
  }

  fn read(&self, max_bytes: u64) -> Result<Zeroizing<Vec<u8>>, AppError> {
    read_file(self, max_bytes)
  }

  /// Writes crash-safely, as a file only the user can read (mode
  /// `0600`): [`storage::atomic_write`].
  fn write(&self, bytes: &[u8]) -> Result<(), AppError> {
    storage::atomic_write(self, bytes).map_err(|source| AppError::FileWrite {
      path: self.clone(),
      source,
    })
  }
}

/// Reads a file the user picked, refusing anything that is not a regular
/// file or is larger than `max_bytes` before reading it — and reading at
/// most one byte more, in case it grows meanwhile (the importers then
/// refuse it). The bytes may be an unencrypted backup, so they are
/// zeroized when dropped.
///
/// The file is opened non-blocking, so that a FIFO in its place cannot
/// hang the read waiting for a writer; for a regular file the flag
/// changes nothing. Its type is then checked on the open file itself,
/// so it cannot be swapped between the check and the read.
///
/// # Errors
///
/// Returns [`AppError::FileRead`], [`AppError::NotAFile`], or
/// `FileTooLarge`.
pub fn read_file(path: &Path, max_bytes: u64) -> Result<Zeroizing<Vec<u8>>, AppError> {
  let read_error = |source| AppError::FileRead {
    path: path.to_owned(),
    source,
  };
  let file = File::from(
    rustix::fs::open(
      path,
      OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
      Mode::empty(),
    )
    .map_err(|errno| read_error(errno.into()))?,
  );
  read_regular_file(&file, path, max_bytes)
}

/// Reads an open file, which must be a regular file: see [`read_file`].
fn read_regular_file(
  file: &File,
  path: &Path,
  max_bytes: u64,
) -> Result<Zeroizing<Vec<u8>>, AppError> {
  let read_error = |source| AppError::FileRead {
    path: path.to_owned(),
    source,
  };
  let metadata = file.metadata().map_err(read_error)?;
  if !metadata.is_file() {
    return Err(AppError::NotAFile(path.to_owned()));
  }
  if metadata.len() > max_bytes {
    return Err(InteropError::FileTooLarge.into());
  }

  // Sized up front: growing the buffer would leave copies of the
  // contents behind in freed memory.
  let capacity = usize::try_from(metadata.len()).map_err(|_| InteropError::FileTooLarge)?;
  let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
  file
    .take(max_bytes + 1)
    .read_to_end(&mut bytes)
    .map_err(read_error)?;
  Ok(bytes)
}

#[cfg(target_os = "android")]
mod document {
  use std::io::{self, Write};

  use super::{AppError, InteropError, PathBuf, Read, UserFile, Zeroizing, read_regular_file};
  use crate::{
    android::{self, DocumentMode, JavaError},
    file_dialog::Document,
  };

  fn io_error(error: JavaError) -> io::Error {
    io::Error::other(error)
  }

  impl UserFile for Document {
    fn name(&self) -> Option<String> {
      self.name.clone()
    }

    /// The name, or else the URI.
    fn location(&self) -> PathBuf {
      PathBuf::from(self.name.as_deref().unwrap_or(&self.uri))
    }

    /// Reads the document through the descriptor its provider opens. A
    /// local document's is a regular file, read as [`read_file`] reads
    /// one; a provider that streams the document (a cloud drive's, for
    /// one) is read up to the same limit as it comes, into a buffer that
    /// grows as it does, and so may leave copies of the contents behind
    /// in freed memory.
    fn read(&self, max_bytes: u64) -> Result<Zeroizing<Vec<u8>>, AppError> {
      let read_error = |source| AppError::FileRead {
        path: self.location(),
        source,
      };
      let file = android::open_document(&self.uri, DocumentMode::Read)
        .map_err(|error| read_error(io_error(error)))?;
      let is_file = file.metadata().map_err(read_error)?.is_file();
      if is_file {
        return read_regular_file(&file, &self.location(), max_bytes);
      }
      let mut bytes = Zeroizing::new(Vec::new());
      file
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
      if u64::try_from(bytes.len()).map_or(true, |len| len > max_bytes) {
        return Err(InteropError::FileTooLarge.into());
      }
      Ok(bytes)
    }

    /// Replaces the document's contents in place: a document provider
    /// offers no atomic replacement, and decides the permissions itself.
    fn write(&self, bytes: &[u8]) -> Result<(), AppError> {
      let write_error = |source| AppError::FileWrite {
        path: self.location(),
        source,
      };
      let mut file = android::open_document(&self.uri, DocumentMode::Replace)
        .map_err(|error| write_error(io_error(error)))?;
      file.write_all(bytes).map_err(write_error)?;
      file.sync_all().map_err(write_error)
    }
  }
}
