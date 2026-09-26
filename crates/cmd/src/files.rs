//! Reading the files the user picks in a file dialog.

use std::{fs::File, io::Read, path::Path};

use interop::InteropError;
use rustix::fs::{Mode, OFlags};
use zeroize::Zeroizing;

use crate::error::AppError;

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
