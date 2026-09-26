//! Importing other authenticators' backups, and exporting to Aegis.
//!
//! The file to import is picked in a native dialog, and its path never
//! leaves Rust: the frontend gets a token for it, so it can retry with a
//! password without the user picking the file again, but it cannot make
//! the app read any other file.

use std::path::{Path, PathBuf};

use interop::{ImportOutcome, InteropError, KnownAccounts, MAX_IMPORT_BYTES, aegis, twofas};
use storage::VaultStorage;
use uuid::Uuid;
use vault_core::Entry;

use super::{AppState, entries::new_entry, lock};
use crate::{
  clock::unix_now,
  dto::{ImportFormat, ImportSummary, PickedFile, SecretString, SkippedEntry},
  error::AppError,
  files::read_file,
  password,
};

/// The file the user picked for import, until it has been imported.
#[derive(Debug, Clone)]
pub(super) struct PendingImport {
  token: Uuid,
  path: PathBuf,
  format: ImportFormat,
}

impl ImportFormat {
  /// The format of a picked backup, going by its extension (in any case):
  /// `.json` is Aegis', `.2fas` is 2FAS'.
  #[must_use]
  pub fn from_path(path: &Path) -> Option<Self> {
    let extension = path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("json") {
      Some(Self::Aegis)
    } else if extension.eq_ignore_ascii_case("2fas") {
      Some(Self::Twofas)
    } else {
      None
    }
  }

  fn import(
    self,
    file: &[u8],
    password: Option<&str>,
    known: &KnownAccounts,
  ) -> Result<ImportOutcome, InteropError> {
    match self {
      Self::Aegis => aegis::import(file, password, known),
      Self::Twofas => twofas::import(file, password, known),
    }
  }
}

impl AppState {
  /// Remembers `path`, just picked by the user, as the file to import,
  /// replacing any earlier pick. Returns the token to import it with.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::UnsupportedFileType`] if the file's extension
  /// is not a backup format's (the dialog can be told any name).
  pub fn pick_import(&self, path: PathBuf) -> Result<PickedFile, AppError> {
    let format =
      ImportFormat::from_path(&path).ok_or_else(|| AppError::UnsupportedFileType(path.clone()))?;
    Ok(self.register_import(path, format))
  }

  fn register_import(&self, path: PathBuf, format: ImportFormat) -> PickedFile {
    self.record_activity();
    let token = Uuid::now_v7();
    let file_name = path
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_default();
    *lock(&self.pending_import) = Some(PendingImport {
      token,
      path,
      format,
    });
    PickedFile {
      token: token.to_string(),
      file_name,
      format,
    }
  }

  /// Imports the file registered under `token`, decrypting it with
  /// `password` if it is encrypted, and adds its accounts to the vault in
  /// one save. Accounts already in the vault, or repeated in the file,
  /// are reported as duplicates rather than added.
  ///
  /// After a success the token is spent. After a failure it stays valid,
  /// so the import can be retried — typically with a password after
  /// [`InteropError::PasswordRequired`].
  ///
  /// # Errors
  ///
  /// Returns [`AppError::NoPendingImport`] if `token` is not the latest
  /// pick, [`AppError::Locked`], [`AppError::NotAFile`],
  /// [`AppError::FileRead`], an [`AppError::Transfer`] error if the file
  /// is too large, not in the format, or does not decrypt, or an error
  /// if the vault cannot be saved.
  pub fn import_file(
    &self,
    token: &str,
    password: Option<&SecretString>,
  ) -> Result<ImportSummary, AppError> {
    let pending = self.pending_import(token)?;
    let known: KnownAccounts = {
      let mut session = self.session();
      session.record_activity();
      session.unlocked()?.payload.active_entries().collect()
    };
    let file = read_file(&pending.path, MAX_IMPORT_BYTES)?;
    // Decrypting runs scrypt or PBKDF2; no lock is held meanwhile. An
    // account added in that window could be imported twice — the user
    // would have to add it by hand during their own import.
    let outcome = pending
      .format
      .import(&file, password.map(SecretString::expose), &known)?;
    let summary = self.add_imported(outcome)?;

    let mut pending_import = lock(&self.pending_import);
    if pending_import
      .as_ref()
      .is_some_and(|current| current.token == pending.token)
    {
      *pending_import = None;
    }
    Ok(summary)
  }

  fn pending_import(&self, token: &str) -> Result<PendingImport, AppError> {
    let token = Uuid::parse_str(token).map_err(|_| AppError::NoPendingImport)?;
    lock(&self.pending_import)
      .as_ref()
      .filter(|pending| pending.token == token)
      .cloned()
      .ok_or(AppError::NoPendingImport)
  }

  fn add_imported(&self, outcome: ImportOutcome) -> Result<ImportSummary, AppError> {
    let now = unix_now()?;
    let entries: Vec<Entry> = outcome
      .imported
      .into_iter()
      .map(|account| new_entry(account, now))
      .collect();
    let imported_ids = entries.iter().map(|entry| entry.id.to_string()).collect();

    if !entries.is_empty() {
      let storage = self.storage()?;
      self
        .session()
        .commit(&storage, VaultStorage::save, |payload| {
          payload.entries.extend(entries);
          Ok(())
        })?;
    }

    Ok(ImportSummary {
      imported_ids,
      skipped: outcome.skipped.iter().map(SkippedEntry::from).collect(),
      duplicates: outcome.duplicates.iter().map(SkippedEntry::from).collect(),
    })
  }

  /// Checks what can be checked before asking the user where to save an
  /// export: that the password is strong enough and the vault unlocked.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::WeakPassword`] or [`AppError::Locked`].
  pub fn check_export(&self, password: &SecretString) -> Result<(), AppError> {
    password::ensure_strong(password)?;
    self.session().unlocked().map(|_| ())
  }

  /// Exports every active entry to `path` as an encrypted Aegis vault,
  /// written crash-safely and readable only by the user (mode `0600`).
  ///
  /// # Errors
  ///
  /// Returns [`AppError::WeakPassword`], [`AppError::Locked`], an
  /// [`AppError::Transfer`] error if encryption fails, or
  /// [`AppError::FileWrite`].
  pub fn export_to_aegis(&self, password: &SecretString, path: &Path) -> Result<(), AppError> {
    password::ensure_strong(password)?;
    let entries: Vec<Entry> = {
      let mut session = self.session();
      session.record_activity();
      session
        .unlocked()?
        .payload
        .active_entries()
        .cloned()
        .collect()
    };
    // scrypt; no lock is held meanwhile.
    let file = aegis::export(&entries, password.expose())?;
    storage::atomic_write(path, &file).map_err(|source| AppError::FileWrite {
      path: path.to_owned(),
      source,
    })
  }
}

#[cfg(test)]
mod tests {
  use std::{
    fs::{self, File},
    os::unix::fs::PermissionsExt,
  };

  use rustix::fs::Mode;
  use serde_json::json;
  use vault_core::OtpConfig;

  use super::*;
  use crate::{error::AppErrorKind, state::fixture::*};

  const EXPORT_PASSWORD: &str = "export password";

  fn plain_aegis(entries: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
      "version": 1,
      "header": { "slots": null, "params": null },
      "db": { "version": 3, "entries": entries, "groups": [] },
    }))
    .unwrap()
  }

  fn aegis_entry(issuer: &str, name: &str, secret: &str) -> serde_json::Value {
    json!({
      "type": "totp",
      "name": name,
      "issuer": issuer,
      "info": { "secret": secret, "algo": "SHA1", "digits": 6, "period": 30 },
    })
  }

  fn register(fixture: &Fixture, name: &str, contents: &[u8], format: ImportFormat) -> PickedFile {
    let path = fixture.dir.path().join(name);
    fs::write(&path, contents).unwrap();
    fixture.state.register_import(path, format)
  }

  fn labels(skipped: &[SkippedEntry]) -> Vec<&str> {
    skipped.iter().map(|entry| entry.label.as_str()).collect()
  }

  #[test]
  fn imports_skip_accounts_already_in_the_vault_or_repeated_in_the_file() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    state
      .add_entry_from_uri(&secret(
        "otpauth://totp/GitHub:alice?secret=JBSWY3DPEHPK3PXP",
      ))
      .unwrap();
    let file = plain_aegis(&json!([
      aegis_entry("GitHub", "alice", "JBSWY3DPEHPK3PXP"),
      aegis_entry("GitLab", "bob", "GEZDGNBVGY3TQOJQ"),
      aegis_entry("GitLab", "bob", "GEZDGNBVGY3TQOJQ"),
      aegis_entry("Broken", "carol", "not base32!"),
    ]));
    let picked = register(&fixture, "aegis.json", &file, ImportFormat::Aegis);
    assert_eq!(picked.file_name, "aegis.json");

    let summary = state.import_file(&picked.token, None).unwrap();

    assert_eq!(summary.imported_ids.len(), 1);
    assert_eq!(labels(&summary.skipped), ["Broken (carol)"]);
    assert_eq!(
      summary
        .duplicates
        .iter()
        .map(|entry| entry.reason.as_str())
        .collect::<Vec<_>>(),
      ["already in the vault", "appears more than once in the file"]
    );
    assert_eq!(state.list_entries().unwrap().len(), 2);

    let again = state.import_file(&picked.token, None).unwrap_err();
    assert_eq!(
      again.kind(),
      AppErrorKind::NoPendingImport,
      "the token is spent"
    );
  }

  #[test]
  fn an_encrypted_import_is_retried_with_the_same_token() {
    let source = Fixture::unlocked();
    source
      .state
      .add_entry_from_uri(&secret(
        "otpauth://hotp/Bank:alice?secret=JBSWY3DPEHPK3PXP&counter=7",
      ))
      .unwrap();
    let export_path = source.dir.path().join("export.json");
    source
      .state
      .export_to_aegis(&secret(EXPORT_PASSWORD), &export_path)
      .unwrap();

    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let picked = register(
      &fixture,
      "export.json",
      &fs::read(&export_path).unwrap(),
      ImportFormat::Aegis,
    );

    let needs_password = state.import_file(&picked.token, None).unwrap_err();
    assert_eq!(needs_password.kind(), AppErrorKind::PasswordRequired);
    let wrong = state
      .import_file(&picked.token, Some(&secret("wrong password")))
      .unwrap_err();
    assert_eq!(wrong.kind(), AppErrorKind::WrongPasswordOrCorrupted);

    let summary = state
      .import_file(&picked.token, Some(&secret(EXPORT_PASSWORD)))
      .unwrap();
    assert_eq!(summary.imported_ids.len(), 1);
    let session = state.session();
    let entry = session
      .unlocked()
      .unwrap()
      .payload
      .active_entries()
      .next()
      .cloned()
      .unwrap();
    assert_eq!(entry.issuer.as_deref(), Some("Bank"));
    assert!(matches!(entry.otp, OtpConfig::Hotp { counter: 7, .. }));
  }

  #[test]
  fn a_pick_is_imported_in_the_format_of_its_extension() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;

    let aegis = state.pick_import("/backups/Aegis.JSON".into()).unwrap();
    assert_eq!(aegis.format, ImportFormat::Aegis);
    assert_eq!(aegis.file_name, "Aegis.JSON");
    let twofas = state.pick_import("/backups/phone.2fas".into()).unwrap();
    assert_eq!(twofas.format, ImportFormat::Twofas);

    for path in [
      "/backups/notes.txt",
      "/backups/json",
      "/backups/archive.2fas.gz",
    ] {
      assert_eq!(
        state.pick_import(path.into()).unwrap_err().kind(),
        AppErrorKind::UnsupportedFileType,
        "{path}"
      );
    }
  }

  #[test]
  fn only_the_latest_pick_can_be_imported() {
    let fixture = Fixture::unlocked();
    let file = plain_aegis(&json!([]));
    let first = register(&fixture, "first.json", &file, ImportFormat::Aegis);
    let second = register(&fixture, "second.json", &file, ImportFormat::Aegis);

    for token in [first.token.as_str(), "not-a-token", ""] {
      assert_eq!(
        fixture.state.import_file(token, None).unwrap_err().kind(),
        AppErrorKind::NoPendingImport
      );
    }
    assert!(fixture.state.import_file(&second.token, None).is_ok());
  }

  #[test]
  fn oversized_and_special_files_are_refused_before_reading() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;

    let huge = fixture.dir.path().join("huge.json");
    File::create(&huge)
      .unwrap()
      .set_len(MAX_IMPORT_BYTES + 1)
      .unwrap();
    let picked = state.register_import(huge, ImportFormat::Aegis);
    assert_eq!(
      state.import_file(&picked.token, None).unwrap_err().kind(),
      AppErrorKind::FileTooLarge
    );

    let picked = state.register_import(fixture.dir.path().to_path_buf(), ImportFormat::Twofas);
    assert_eq!(
      state.import_file(&picked.token, None).unwrap_err().kind(),
      AppErrorKind::NotAFile
    );

    let picked = state.register_import("/dev/zero".into(), ImportFormat::Twofas);
    assert_eq!(
      state.import_file(&picked.token, None).unwrap_err().kind(),
      AppErrorKind::NotAFile
    );

    // A FIFO with no writer would block a plain open() forever.
    let fifo = fixture.dir.path().join("fifo");
    rustix::fs::mknodat(
      rustix::fs::CWD,
      &fifo,
      rustix::fs::FileType::Fifo,
      Mode::RUSR | Mode::WUSR,
      0,
    )
    .unwrap();
    let picked = state.register_import(fifo, ImportFormat::Aegis);
    assert_eq!(
      state.import_file(&picked.token, None).unwrap_err().kind(),
      AppErrorKind::NotAFile
    );

    let picked = state.register_import(fixture.dir.path().join("missing"), ImportFormat::Aegis);
    assert_eq!(
      state.import_file(&picked.token, None).unwrap_err().kind(),
      AppErrorKind::FileRead
    );
  }

  #[test]
  fn importing_needs_an_unlocked_vault() {
    let fixture = Fixture::unlocked();
    let picked = register(
      &fixture,
      "a.json",
      &plain_aegis(&json!([])),
      ImportFormat::Aegis,
    );
    fixture.state.lock_vault();

    assert_eq!(
      fixture
        .state
        .import_file(&picked.token, None)
        .unwrap_err()
        .kind(),
      AppErrorKind::Locked
    );
  }

  #[test]
  fn the_export_is_private_and_replaces_an_earlier_one_atomically() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    state
      .add_entry_from_uri(&secret(
        "otpauth://totp/GitHub:alice?secret=JBSWY3DPEHPK3PXP",
      ))
      .unwrap();
    let path = fixture.dir.path().join("aegis.json");
    fs::write(&path, b"an earlier export").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    state
      .export_to_aegis(&secret(EXPORT_PASSWORD), &path)
      .unwrap();

    assert_eq!(
      fs::metadata(&path).unwrap().permissions().mode() & 0o777,
      0o600
    );
    let exported = fs::read(&path).unwrap();
    let outcome =
      aegis::import(&exported, Some(EXPORT_PASSWORD), &KnownAccounts::default()).unwrap();
    assert_eq!(outcome.imported.len(), 1);
  }

  #[test]
  fn exports_need_a_strong_password_and_an_unlocked_vault() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let path = fixture.dir.path().join("aegis.json");

    for error in [
      state.check_export(&secret("short")).unwrap_err(),
      state.export_to_aegis(&secret("short"), &path).unwrap_err(),
    ] {
      assert_eq!(error.kind(), AppErrorKind::WeakPassword);
    }
    state.lock_vault();
    assert_eq!(
      state
        .check_export(&secret(EXPORT_PASSWORD))
        .unwrap_err()
        .kind(),
      AppErrorKind::Locked
    );
    assert!(!path.exists());
  }

  #[test]
  fn a_failed_export_write_names_the_file() {
    let fixture = Fixture::unlocked();
    let path = fixture.dir.path().join("missing-directory/aegis.json");

    let error = fixture
      .state
      .export_to_aegis(&secret(EXPORT_PASSWORD), &path)
      .unwrap_err();

    assert_eq!(error.kind(), AppErrorKind::FileWrite);
    assert!(
      error.message().starts_with("could not write "),
      "{}",
      error.message()
    );
  }
}
