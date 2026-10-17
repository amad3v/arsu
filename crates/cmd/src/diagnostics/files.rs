//! The log files: JSON Lines in the app's own log directory, rotated, a
//! header record first.
//!
//! `arsu.jsonl` is the current file; past its size limit it becomes
//! `arsu.1.jsonl`, the older ones move up one (`arsu.2.jsonl`, …) and the
//! oldest beyond the limit is deleted. An app run's records follow its
//! environment record, written before the run's first record in each
//! file: every file reads on its own, every record has what the app and
//! the device were when it was made (an update may come between two
//! runs), and a run that records nothing writes nothing. Each record is
//! written and flushed whole: a crash leaves at most its own last line
//! unfinished.

use std::{
  fs::{self, File, OpenOptions},
  io::{self, Write},
  path::{Path, PathBuf},
};

use crate::error::AppError;

/// The current file's name.
pub const CURRENT: &str = "arsu.jsonl";

/// The text logs of earlier versions (`arsu.log`, `arsu_<date>.log`),
/// deleted when the log is opened.
const LEGACY_PREFIX: &str = "arsu";
const LEGACY_EXTENSION: &str = "log";

/// A log file as it was read, for a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFile {
  /// Its name in the log directory.
  pub name: String,
  pub bytes: Vec<u8>,
}

impl LogFile {
  /// Its records, as text: a line cut short by a crash reads as what is
  /// left of it.
  #[must_use]
  pub fn text(&self) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(&self.bytes)
  }
}

/// The log files, written one record at a time.
#[derive(Debug)]
pub struct LogFiles {
  dir: PathBuf,
  /// This run's environment record, written before its first record in
  /// each file.
  header: String,
  /// Whether the current file has it yet.
  header_written: bool,
  max_bytes: u64,
  /// Files kept, the current one included.
  kept: usize,
  file: File,
  size: u64,
}

impl LogFiles {
  /// Opens the log in `dir` (created if missing) for a new app run, whose
  /// environment record is `header`: deletes earlier versions' text
  /// logs.
  ///
  /// # Errors
  ///
  /// If the directory or the current file cannot be created or opened.
  pub fn open(dir: &Path, header: String, max_bytes: u64, kept: usize) -> io::Result<Self> {
    fs::create_dir_all(dir)?;
    remove_legacy(dir);
    let (file, size) = open_current(dir)?;
    Ok(Self {
      dir: dir.to_owned(),
      header,
      header_written: false,
      max_bytes,
      kept: kept.max(1),
      file,
      size,
    })
  }

  /// Writes `line`, one record: in a new file if it would not fit in this
  /// one, after this run's environment record if the file lacks it.
  ///
  /// # Errors
  ///
  /// If the file cannot be written or rotated.
  pub fn write(&mut self, line: &str) -> io::Result<()> {
    let header = if self.header_written {
      0
    } else {
      self.header.len() as u64 + 1
    };
    if self.size > 0 && self.size + header + line.len() as u64 + 1 > self.max_bytes {
      self.rotate()?;
    }
    if !self.header_written {
      self.write_line(&self.header.clone())?;
      self.header_written = true;
    }
    self.write_line(line)
  }

  /// Flushes what was written.
  ///
  /// # Errors
  ///
  /// If the file cannot be flushed.
  pub fn flush(&mut self) -> io::Result<()> {
    self.file.flush()
  }

  /// Every log file, oldest first, as written so far.
  ///
  /// # Errors
  ///
  /// [`AppError::FileRead`] if one cannot be read.
  pub fn read_all(&mut self) -> Result<Vec<LogFile>, AppError> {
    self.flush().map_err(|source| AppError::FileRead {
      path: self.dir.join(CURRENT),
      source,
    })?;
    oldest_first(&self.dir)
      .into_iter()
      .map(|path| {
        let bytes = fs::read(&path).map_err(|source| AppError::FileRead {
          path: path.clone(),
          source,
        })?;
        let name = path
          .file_name()
          .map(|name| name.to_string_lossy().into_owned())
          .unwrap_or_default();
        Ok(LogFile { name, bytes })
      })
      .collect()
  }

  fn write_line(&mut self, line: &str) -> io::Result<()> {
    let mut record = Vec::with_capacity(line.len() + 1);
    record.extend_from_slice(line.as_bytes());
    record.push(b'\n');
    self.file.write_all(&record)?;
    self.file.flush()?;
    self.size += record.len() as u64;
    Ok(())
  }

  fn rotate(&mut self) -> io::Result<()> {
    for older in (1..self.kept).rev() {
      let from = rotated(&self.dir, older);
      if older + 1 >= self.kept {
        remove_if_present(&from)?;
      } else if from.exists() {
        fs::rename(&from, rotated(&self.dir, older + 1))?;
      }
    }
    if self.kept > 1 {
      fs::rename(self.dir.join(CURRENT), rotated(&self.dir, 1))?;
    } else {
      remove_if_present(&self.dir.join(CURRENT))?;
    }
    let (file, size) = open_current(&self.dir)?;
    self.file = file;
    self.size = size;
    self.header_written = false;
    Ok(())
  }
}

/// The log files in `dir`, oldest first.
#[must_use]
fn oldest_first(dir: &Path) -> Vec<PathBuf> {
  let mut files: Vec<(usize, PathBuf)> = fs::read_dir(dir)
    .into_iter()
    .flatten()
    .flatten()
    .filter_map(|entry| {
      let name = entry.file_name().to_string_lossy().into_owned();
      let age = if name == CURRENT {
        0
      } else {
        name
          .strip_prefix("arsu.")?
          .strip_suffix(".jsonl")?
          .parse()
          .ok()?
      };
      Some((age, entry.path()))
    })
    .collect();
  files.sort_by_key(|(age, _)| std::cmp::Reverse(*age));
  files.into_iter().map(|(_, path)| path).collect()
}

fn rotated(dir: &Path, age: usize) -> PathBuf {
  dir.join(format!("arsu.{age}.jsonl"))
}

fn open_current(dir: &Path) -> io::Result<(File, u64)> {
  let file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(dir.join(CURRENT))?;
  let size = file.metadata()?.len();
  Ok((file, size))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
  match fs::remove_file(path) {
    Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
    _ => Ok(()),
  }
}

/// Deletes the text logs earlier versions wrote; best effort.
fn remove_legacy(dir: &Path) {
  for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
    let path = entry.path();
    let legacy = path.extension().is_some_and(|ext| ext == LEGACY_EXTENSION)
      && entry
        .file_name()
        .to_string_lossy()
        .starts_with(LEGACY_PREFIX);
    if legacy {
      let _ = fs::remove_file(path);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
      .unwrap()
      .lines()
      .map(str::to_owned)
      .collect()
  }

  #[test]
  fn a_run_writes_its_header_before_its_first_record_and_nothing_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = LogFiles::open(dir.path(), "H1".to_owned(), 1_000, 5).unwrap();
    first.write("a").unwrap();
    first.write("b").unwrap();
    drop(first);
    drop(LogFiles::open(dir.path(), "H2".to_owned(), 1_000, 5).unwrap());

    let mut third = LogFiles::open(dir.path(), "H3".to_owned(), 1_000, 5).unwrap();
    third.write("c").unwrap();

    assert_eq!(
      lines(&dir.path().join(CURRENT)),
      ["H1", "a", "b", "H3", "c"],
      "the second run recorded nothing"
    );
  }

  #[test]
  fn a_full_file_rotates_and_the_new_one_starts_with_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let mut files = LogFiles::open(dir.path(), "H".to_owned(), 10, 3).unwrap();

    for record in ["111111", "222222", "333333", "444444"] {
      files.write(record).unwrap();
    }

    assert_eq!(
      oldest_first(dir.path())
        .iter()
        .map(|path| lines(path))
        .collect::<Vec<_>>(),
      [
        vec!["H", "222222"],
        vec!["H", "333333"],
        vec!["H", "444444"]
      ],
      "the oldest beyond three files is gone"
    );
  }

  #[test]
  fn a_record_longer_than_a_file_still_goes_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut files = LogFiles::open(dir.path(), "H".to_owned(), 4, 2).unwrap();

    files.write("a long record").unwrap();

    assert_eq!(lines(&dir.path().join(CURRENT)), ["H", "a long record"]);
  }

  #[test]
  fn earlier_text_logs_are_deleted_and_other_files_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("arsu.log"), "old").unwrap();
    fs::write(dir.path().join("arsu_2026-10-09_09-00-00.log"), "old").unwrap();
    fs::write(dir.path().join("notes.txt"), "mine").unwrap();

    LogFiles::open(dir.path(), "H".to_owned(), 1_000, 5)
      .unwrap()
      .write("a")
      .unwrap();

    let mut names: Vec<String> = fs::read_dir(dir.path())
      .unwrap()
      .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
      .collect();
    names.sort();
    assert_eq!(names, ["arsu.jsonl", "notes.txt"]);
  }
}
