//! Vault file I/O: locking, crash-safe writes, backup rotation, and
//! upgrading a vault's key derivation.
//!
//! `vault-core` defines what the bytes mean; `crypto` turns plaintext
//! into ciphertext and back. This module's job is getting those bytes
//! to and from disk without ever leaving the vault missing or
//! half-written, even across a crash or power loss mid-write.

use std::{
  fs::{self, File, OpenOptions, TryLockError},
  io,
  os::unix::fs::OpenOptionsExt,
  path::{Path, PathBuf},
  sync::{Mutex, MutexGuard, PoisonError},
  time::{Duration, SystemTime, UNIX_EPOCH},
};

use ctutils::CtEq;

use crypto::{KdfParams, MasterKey, Nonce, Salt, decrypt, derive_key, encrypt, generate_salt};
use vault_core::{EnvelopeHeader, VaultEnvelope, VaultPayload};

use crate::{StorageError, fs_util};

/// How many previous versions of the vault to keep as a local safety
/// net. Not a security feature — just insurance against an accidental
/// bad edit.
pub const MAX_BACKUPS: usize = 3;

/// An unlocked vault: the decrypted payload plus everything needed to
/// re-encrypt and save it, bundled together so callers don't have to
/// juggle the key/salt/KDF-params as separate arguments on every save.
///
/// `payload` is public and mutable — edit it directly (add/edit/remove
/// entries), then call [`VaultStorage::save`]. The key stays private:
/// it never leaves this struct except by being compared (in constant
/// time) against a candidate key.
///
/// Dropping an `UnlockedVault` zeroizes the key and every seed in the
/// payload.
pub struct UnlockedVault {
  pub payload: VaultPayload,
  key: MasterKey,
  salt: Salt,
  kdf_params: KdfParams,
}

impl UnlockedVault {
  /// The salt the vault key was derived with. Not secret.
  #[must_use]
  pub fn salt(&self) -> Salt {
    self.salt
  }

  /// The KDF parameters the vault key was derived with. Not secret.
  #[must_use]
  pub fn kdf_params(&self) -> KdfParams {
    self.kdf_params
  }

  /// Whether `candidate` is this vault's key, compared in constant time.
  ///
  /// Together with [`Self::salt`] and [`Self::kdf_params`] this lets a
  /// caller re-verify the master password without holding whatever
  /// lock guards the vault for the ~0.5 s the KDF takes: copy the salt
  /// and parameters out, run [`crypto::derive_key`] unlocked, then
  /// compare.
  #[must_use]
  pub fn key_matches(&self, candidate: &MasterKey) -> bool {
    self.key.ct_eq(candidate).to_bool()
  }

  /// Re-derive the key from `password` with this vault's salt and
  /// parameters and compare it with the vault's key in constant time.
  /// Runs the full KDF; see [`Self::key_matches`] for doing that
  /// without borrowing the vault meanwhile.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::Crypto`] if key derivation fails.
  pub fn verify_password(&self, password: &[u8]) -> Result<bool, StorageError> {
    let candidate = derive_key(password, &self.salt, &self.kdf_params)?;
    Ok(self.key_matches(&candidate))
  }

  /// Encrypt the payload into a complete vault file: a fresh nonce, the
  /// header as associated data, the current format version.
  fn seal(&self) -> Result<Vec<u8>, StorageError> {
    let plaintext = self.payload.to_bytes()?; // zeroized on drop
    let nonce = Nonce::generate()?;
    let header = EnvelopeHeader {
      kdf_params: self.kdf_params,
      salt: self.salt,
      nonce: *nonce.as_bytes(),
    };
    let ciphertext = encrypt(&self.key, nonce, &plaintext, &header.to_bytes())?;
    Ok(header.encode(&ciphertext))
  }
}

/// Whether a save shifts the backup ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backups {
  Rotate,
  Keep,
}

/// Handle on one vault file.
///
/// Constructing it takes an exclusive advisory lock (`flock`) on
/// `<vault>.lock` next to the vault, held until the handle is dropped:
/// a second instance of the app gets [`StorageError::InUse`] instead of
/// silently overwriting the first one's edits.
///
/// Every method takes `&self` and writes are serialised internally, so
/// one handle can be shared between threads (e.g. in an `Arc`) and the
/// slow operations — [`Self::create`] and [`Self::open`], which run the
/// KDF — can run without holding any lock the caller keeps around its
/// unlocked vault.
///
/// The vault's directory is treated as private to it: it is created
/// (or tightened to) mode `0700`, and must be on a filesystem with hard
/// links (every Linux-native one) — backups are hard links.
pub struct VaultStorage {
  vault_path: PathBuf,
  kdf_params: KdfParams,
  write_lock: Mutex<()>,
  /// Held for its `flock`; released when the handle drops.
  _lock_file: File,
}

impl VaultStorage {
  /// Take ownership of the vault at `vault_path`: create its directory
  /// if needed (mode `0700`), lock it against other instances, and
  /// delete temporary files left by writes that were interrupted.
  ///
  /// New vaults are created with [`KdfParams::RECOMMENDED`], and vaults
  /// with weaker parameters are upgraded to them on unlock; see
  /// [`Self::with_kdf_params`].
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::InUse`] if another handle holds the lock,
  /// or [`StorageError::Io`] if `vault_path` has no parent directory or
  /// the directory or lock file cannot be set up.
  pub fn new(vault_path: PathBuf) -> Result<Self, StorageError> {
    let dir = fs_util::parent_dir(&vault_path).map_err(StorageError::io(&vault_path))?;
    fs_util::ensure_private_dir(dir).map_err(StorageError::io(dir))?;
    let lock_file = lock(&vault_path)?;
    // With the lock held no other writer exists, so any temp file next
    // to the vault belongs to a save that never committed.
    fs_util::remove_stale_temp_files(&vault_path).map_err(StorageError::io(dir))?;

    Ok(Self {
      vault_path,
      kdf_params: KdfParams::RECOMMENDED,
      write_lock: Mutex::new(()),
      _lock_file: lock_file,
    })
  }

  /// Use `kdf_params` for new vaults and as the target of the
  /// upgrade-on-unlock in [`Self::open`]. Tests use
  /// [`KdfParams::MINIMUM`] to keep the KDF fast.
  #[must_use]
  pub fn with_kdf_params(mut self, kdf_params: KdfParams) -> Self {
    self.kdf_params = kdf_params;
    self
  }

  #[must_use]
  pub fn path(&self) -> &Path {
    &self.vault_path
  }

  /// # Errors
  ///
  /// Returns [`StorageError::Io`] if it cannot be determined whether the
  /// vault exists (e.g. a permission error), rather than guessing "no".
  pub fn exists(&self) -> Result<bool, StorageError> {
    self
      .vault_path
      .try_exists()
      .map_err(StorageError::io(&self.vault_path))
  }

  /// Create a brand-new, empty vault encrypted under `master_password`.
  /// Never overwrites an existing vault — the file is published with an
  /// atomic no-clobber rename, not after an existence check.
  ///
  /// If no vault exists at this path but `.bak.N` files do — left behind
  /// by a vault that went missing some other way than a save, such as an
  /// accidental delete or a sync tool — they are moved aside to
  /// `<vault>.orphaned-<unix-secs>.bak.N` before the new vault is
  /// published, so the next few rotating saves can no longer prune them
  /// out of existence. Nothing yet reads or restores
  /// an orphaned backup; recovering one is a manual step for now.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::AlreadyExists`] if a vault already exists,
  /// or an error if salt generation, key derivation, encryption,
  /// orphaning old backups, or writing the file fails.
  pub fn create(&self, master_password: &[u8]) -> Result<UnlockedVault, StorageError> {
    let salt = generate_salt()?;
    let vault = UnlockedVault {
      payload: VaultPayload::new(),
      key: derive_key(master_password, &salt, &self.kdf_params)?,
      salt,
      kdf_params: self.kdf_params,
    };
    let bytes = vault.seal()?;

    let _guard = self.write_guard();
    let staged =
      fs_util::stage(&self.vault_path, &bytes).map_err(StorageError::io(&self.vault_path))?;
    self.orphan_backups_left_by_a_missing_vault()?;
    staged.commit_new().map_err(|e| {
      if e.kind() == io::ErrorKind::AlreadyExists {
        StorageError::AlreadyExists(self.vault_path.clone())
      } else {
        StorageError::io(&self.vault_path)(e)
      }
    })?;
    Ok(vault)
  }

  /// If a vault is already at [`Self::vault_path`], any `.bak.N` next to
  /// it belong to that vault and [`Self::create`] is about to (correctly)
  /// refuse to replace it — leave them alone. Otherwise, any `.bak.N`
  /// found are orphans of a vault that went missing without going
  /// through this handle, so move each aside under a name
  /// [`Self::rotate_backups`] never touches: a no-clobber hard link to
  /// `<vault>.orphaned-<unix-secs>.bak.N`, then remove the original.
  fn orphan_backups_left_by_a_missing_vault(&self) -> Result<(), StorageError> {
    if self.exists()? {
      return Ok(());
    }
    let unix_secs = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      // Never in practice: the clock is not set before 1970. Falling
      // back to a fixed suffix is still safe, since the no-clobber hard
      // link below fails loudly on a name collision rather than quietly
      // overwriting an earlier orphaned backup.
      .unwrap_or(Duration::ZERO)
      .as_secs();

    for n in 1..=MAX_BACKUPS {
      let backup = self.backup_path(n);
      let orphaned = self.orphaned_backup_path(unix_secs, n);
      match fs::hard_link(&backup, &orphaned) {
        Ok(()) => fs_util::remove_if_exists(&backup).map_err(StorageError::io(&backup))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {} // no backup in this slot
        Err(e) => return Err(StorageError::io(&backup)(e)),
      }
    }
    Ok(())
  }

  /// Unlock the vault: read, validate the header, derive the key,
  /// decrypt, parse. Format-version-1 vaults open as well; the next
  /// save rewrites them in the current format.
  ///
  /// If the vault's KDF parameters are weaker than this handle's (see
  /// [`KdfParams::is_weaker_than`]), the key is re-derived with a fresh
  /// salt and the stronger parameters, the vault is saved, and its
  /// backups — still encrypted under the weaker parameters — are
  /// deleted. That runs the KDF a second time, once.
  ///
  /// # Errors
  ///
  /// Returns [`StorageError::NotFound`] if there is no vault,
  /// [`StorageError::VaultCore`] if the file is not a supported vault
  /// (including out-of-bounds KDF parameters, rejected before any
  /// derivation), [`StorageError::Crypto`] for a wrong password or a
  /// tampered file, or [`StorageError::Io`] if reading, or saving the
  /// upgraded vault, fails.
  pub fn open(&self, master_password: &[u8]) -> Result<UnlockedVault, StorageError> {
    let bytes = fs::read(&self.vault_path).map_err(|e| {
      if e.kind() == io::ErrorKind::NotFound {
        StorageError::NotFound(self.vault_path.clone())
      } else {
        StorageError::io(&self.vault_path)(e)
      }
    })?;
    let envelope = VaultEnvelope::from_bytes(&bytes)?;
    let header = envelope.header();

    let key = derive_key(master_password, &header.salt, &header.kdf_params)?;
    let plaintext = decrypt(
      &key,
      &header.nonce,
      envelope.ciphertext(),
      &envelope.associated_data(),
    )?;
    let vault = UnlockedVault {
      payload: VaultPayload::from_bytes(&plaintext)?,
      key,
      salt: header.salt,
      kdf_params: header.kdf_params,
    };

    if vault.kdf_params.is_weaker_than(&self.kdf_params) {
      self.upgrade_kdf(vault, master_password)
    } else {
      Ok(vault)
    }
  }

  /// Re-encrypt and save, first shifting the backup ring so the current
  /// file becomes `.bak.1`. Use this for edits a user might want to
  /// undo — adding, deleting or importing entries.
  ///
  /// Reuses the key already derived by [`Self::open`]/[`Self::create`];
  /// re-running Argon2id on every save would make routine edits take
  /// as long as unlocking.
  ///
  /// # Errors
  ///
  /// Returns an error if the payload cannot be serialized or encrypted,
  /// or the file cannot be written. On error the vault file is
  /// unchanged.
  pub fn save(&self, vault: &UnlockedVault) -> Result<(), StorageError> {
    self.write(vault, Backups::Rotate)
  }

  /// Re-encrypt and save without touching the backups. Use this for
  /// bookkeeping that is not an edit anyone would roll back — notably
  /// advancing an HOTP counter on every reveal — so it doesn't push the
  /// snapshots of real edits out of the [`MAX_BACKUPS`]-deep ring. Just
  /// as crash-safe as [`Self::save`].
  ///
  /// # Errors
  ///
  /// As for [`Self::save`].
  pub fn save_without_rotation(&self, vault: &UnlockedVault) -> Result<(), StorageError> {
    self.write(vault, Backups::Keep)
  }

  fn write(&self, vault: &UnlockedVault, backups: Backups) -> Result<(), StorageError> {
    let bytes = vault.seal()?;
    let _guard = self.write_guard();
    self.replace(&bytes, backups)
  }

  /// Replace the vault file with `bytes` such that the vault path holds
  /// either the old or the new complete file at every instant:
  ///
  /// 1. the new contents are written and fsynced to a temp file — a
  ///    failure here (full disk, crash) leaves everything untouched;
  /// 2. the backups shift and the current vault is *hard-linked* to
  ///    `.bak.1` — the vault path itself is never removed;
  /// 3. `rename(2)` atomically swaps the temp file in, and the directory
  ///    fsync makes the links and renames durable.
  ///
  /// Callers hold the write guard.
  fn replace(&self, bytes: &[u8], backups: Backups) -> Result<(), StorageError> {
    let staged =
      fs_util::stage(&self.vault_path, bytes).map_err(StorageError::io(&self.vault_path))?;
    if backups == Backups::Rotate {
      self.rotate_backups()?;
    }
    staged.commit().map_err(StorageError::io(&self.vault_path))
  }

  /// `.bak.(N-1)` → `.bak.N` … `.bak.1` → `.bak.2`, dropping the oldest,
  /// then hard-link the current vault as `.bak.1`.
  fn rotate_backups(&self) -> Result<(), StorageError> {
    let oldest = self.backup_path(MAX_BACKUPS);
    fs_util::remove_if_exists(&oldest).map_err(StorageError::io(&oldest))?;

    for n in (1..MAX_BACKUPS).rev() {
      let from = self.backup_path(n);
      match fs::rename(&from, self.backup_path(n + 1)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {} // slot not filled yet
        result => result.map_err(StorageError::io(&from))?,
      }
    }

    let newest = self.backup_path(1);
    match fs::hard_link(&self.vault_path, &newest) {
      Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()), // nothing saved yet
      result => result.map_err(StorageError::io(&newest)),
    }
  }

  /// Re-key `vault` under a fresh salt and this handle's parameters,
  /// save it, and delete the backups, which are still sealed under the
  /// weaker parameters and would otherwise remain the cheaper target
  /// for an offline guessing attack.
  fn upgrade_kdf(
    &self,
    vault: UnlockedVault,
    master_password: &[u8],
  ) -> Result<UnlockedVault, StorageError> {
    let salt = generate_salt()?;
    let upgraded = UnlockedVault {
      key: derive_key(master_password, &salt, &self.kdf_params)?,
      salt,
      kdf_params: self.kdf_params,
      payload: vault.payload,
    };
    let bytes = upgraded.seal()?;

    let _guard = self.write_guard();
    self.replace(&bytes, Backups::Keep)?;
    for n in 1..=MAX_BACKUPS {
      let backup = self.backup_path(n);
      fs_util::remove_if_exists(&backup).map_err(StorageError::io(&backup))?;
    }
    fs_util::sync_parent(&self.vault_path).map_err(StorageError::io(&self.vault_path))?;
    Ok(upgraded)
  }

  /// `<vault file name>.bak.<n>`, built on the `OsStr` so any file name
  /// works, UTF-8 or not.
  fn backup_path(&self, n: usize) -> PathBuf {
    self.vault_path.with_added_extension(format!("bak.{n}"))
  }

  /// `<vault file name>.orphaned-<unix_secs>.bak.<n>` — where
  /// [`Self::orphan_backups_left_by_a_missing_vault`] moves a `.bak.n`
  /// that no longer has a vault to go with it.
  fn orphaned_backup_path(&self, unix_secs: u64, n: usize) -> PathBuf {
    self
      .vault_path
      .with_added_extension(format!("orphaned-{unix_secs}.bak.{n}"))
  }

  /// Serialises this process's writes. The guarded data is `()`, so a
  /// panic while holding it cannot leave anything inconsistent and a
  /// poisoned lock is safe to reuse.
  fn write_guard(&self) -> MutexGuard<'_, ()> {
    self
      .write_lock
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
  }
}

/// Open (creating if needed) `<vault>.lock` and take an exclusive,
/// non-blocking `flock` on it.
fn lock(vault_path: &Path) -> Result<File, StorageError> {
  let lock_path = vault_path.with_added_extension("lock");
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .mode(0o600)
    .open(&lock_path)
    .map_err(StorageError::io(&lock_path))?;

  match file.try_lock() {
    Ok(()) => Ok(file),
    Err(TryLockError::WouldBlock) => Err(StorageError::InUse(vault_path.to_path_buf())),
    Err(TryLockError::Error(source)) => Err(StorageError::Io {
      path: lock_path,
      source,
    }),
  }
}

#[cfg(test)]
mod tests {
  use std::{
    ffi::OsStr,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
  };

  use uuid::Uuid;

  use crypto::CryptoError;
  use otp::Algorithm;
  use vault_core::{CURRENT_FORMAT_VERSION, Entry, HEADER_LEN, OtpConfig, VaultCoreError};

  use super::*;

  const PASSWORD: &[u8] = b"correct horse battery staple";

  /// A vault written by the pre-envelope-v2 code (format version 1,
  /// KDF 19 MiB / t=2 / p=1 — the old debug-build default), with
  /// password [`PASSWORD`] and three entries: a TOTP ("Example",
  /// "alice@example.com"), an HOTP ("bob", counter 7), and a tombstoned
  /// TOTP ("Gone", "carol").
  const V1_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/vault-v1.bin");

  fn storage(path: &Path) -> VaultStorage {
    VaultStorage::new(path.to_path_buf())
      .unwrap()
      .with_kdf_params(KdfParams::MINIMUM)
  }

  fn temp_vault() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir for test");
    let path = dir.path().join("vault");
    (dir, path)
  }

  fn entry(label: &str) -> Entry {
    Entry {
      id: Uuid::now_v7(),
      issuer: Some("Example".to_string()),
      account_label: label.to_string(),
      otp: OtpConfig::Totp {
        secret: b"12345678901234567890".to_vec().into(),
        algorithm: Algorithm::Sha1,
        digits: 6,
        period: 30,
      },
      icon: None,
      tags: vec![],
      notes: None,
      created_at: 0,
      updated_at: 0,
      deleted_at: None,
    }
  }

  fn labels(vault: &UnlockedVault) -> Vec<&str> {
    vault
      .payload
      .entries
      .iter()
      .map(|e| e.account_label.as_str())
      .collect()
  }

  fn temp_files(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
      .unwrap()
      .map(|e| e.unwrap().path())
      .filter(|p| p.file_name().unwrap().as_bytes().starts_with(b".vault-"))
      .collect()
  }

  // ---- create / open ---------------------------------------------------

  #[test]
  fn create_then_open_round_trips() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);

    storage.create(PASSWORD).unwrap();

    let unlocked = storage.open(PASSWORD).unwrap();
    assert!(unlocked.payload.entries.is_empty());
    assert_eq!(unlocked.kdf_params(), KdfParams::MINIMUM);
  }

  #[test]
  fn new_vaults_use_the_recommended_kdf_by_default() {
    let (_dir, path) = temp_vault();
    let storage = VaultStorage::new(path).unwrap();
    assert_eq!(storage.kdf_params, KdfParams::RECOMMENDED);
  }

  #[test]
  fn wrong_password_is_rejected() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    storage.create(b"correct password").unwrap();

    assert!(matches!(
      storage.open(b"wrong password"),
      Err(StorageError::Crypto(CryptoError::Decrypt))
    ));
  }

  #[test]
  fn create_refuses_to_overwrite_existing_vault() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    storage.create(PASSWORD).unwrap();
    let before = fs::read(&path).unwrap();

    let result = storage.create(b"a different password");

    assert!(matches!(result, Err(StorageError::AlreadyExists(_))));
    assert_eq!(fs::read(&path).unwrap(), before);
  }

  #[test]
  fn create_never_clobbers_a_file_that_appeared_meanwhile() {
    // No existence check to race: whatever is at the vault path when
    // the new file is published is kept.
    let (dir, path) = temp_vault();
    let storage = storage(&path);
    fs::write(&path, b"someone else's vault").unwrap();

    assert!(matches!(
      storage.create(PASSWORD),
      Err(StorageError::AlreadyExists(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), b"someone else's vault");
    assert!(temp_files(dir.path()).is_empty());
  }

  #[test]
  fn open_missing_vault_is_not_found() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);

    assert!(!storage.exists().unwrap());
    assert!(matches!(
      storage.open(PASSWORD),
      Err(StorageError::NotFound(_))
    ));
  }

  #[test]
  fn edits_persist_across_save_and_reopen() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    let mut unlocked = storage.create(PASSWORD).unwrap();

    unlocked.payload.entries.push(entry("me@example.com"));
    storage.save(&unlocked).unwrap();

    let reopened = storage.open(PASSWORD).unwrap();
    assert_eq!(labels(&reopened), ["me@example.com"]);
    assert_eq!(
      reopened.payload.entries[0].otp,
      unlocked.payload.entries[0].otp
    );
  }

  // ---- format versions -------------------------------------------------

  #[test]
  fn a_v1_vault_opens_and_is_rewritten_as_v2_on_the_next_save() {
    let (_dir, path) = temp_vault();
    fs::write(&path, V1_FIXTURE).unwrap();
    let storage = storage(&path);

    let vault = storage.open(PASSWORD).unwrap();
    assert_eq!(labels(&vault), ["alice@example.com", "bob", "carol"]);
    assert_eq!(vault.payload.active_entries().count(), 2);
    match &vault.payload.entries[1].otp {
      OtpConfig::Hotp {
        secret,
        algorithm,
        digits,
        counter,
      } => {
        assert_eq!(secret.expose_secret(), b"12345678901234567890123456789012");
        assert_eq!((*algorithm, *digits, *counter), (Algorithm::Sha256, 8, 7));
      }
      OtpConfig::Totp { .. } => panic!("expected the HOTP entry"),
    }
    assert_eq!(
      fs::read(&path).unwrap(),
      V1_FIXTURE,
      "opening alone does not rewrite the file"
    );

    storage.save(&vault).unwrap();

    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[4..6], &CURRENT_FORMAT_VERSION.to_be_bytes());
    assert_eq!(
      VaultEnvelope::from_bytes(&bytes).unwrap().format_version(),
      CURRENT_FORMAT_VERSION
    );
    let reopened = storage.open(PASSWORD).unwrap();
    assert_eq!(labels(&reopened), labels(&vault));
    assert_eq!(reopened.payload.vault_id, vault.payload.vault_id);
    assert_eq!(fs::read(storage.backup_path(1)).unwrap(), V1_FIXTURE);
  }

  #[test]
  fn tampering_with_any_header_field_fails_to_open() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    storage.create(PASSWORD).unwrap();
    let good = fs::read(&path).unwrap();

    // (offset, what) — p_cost 1 → 2 stays within bounds.
    for (offset, field) in [
      (17, "p_cost"),
      (18, "salt"),
      (34, "nonce"),
      (HEADER_LEN, "ciphertext"),
    ] {
      let mut bad = good.clone();
      bad[offset] ^= 0x03;
      fs::write(&path, &bad).unwrap();
      assert!(
        matches!(
          storage.open(PASSWORD),
          Err(StorageError::Crypto(CryptoError::Decrypt))
        ),
        "tampered {field} must not open"
      );
    }

    let mut downgraded = good.clone();
    downgraded[4..6].copy_from_slice(&1u16.to_be_bytes());
    fs::write(&path, &downgraded).unwrap();
    assert!(storage.open(PASSWORD).is_err(), "version downgrade");

    let mut hostile = good;
    hostile[10..14].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(&path, &hostile).unwrap();
    assert!(
      matches!(
        storage.open(PASSWORD),
        Err(StorageError::VaultCore(
          VaultCoreError::UnsupportedKdfParams(_)
        ))
      ),
      "an unbounded t_cost is rejected before deriving anything"
    );
  }

  // ---- KDF upgrade and password re-verification ----------------------

  #[test]
  fn opening_a_vault_with_weaker_kdf_params_upgrades_it() {
    let (dir, path) = temp_vault();
    let mut vault = storage(&path).create(PASSWORD).unwrap();
    let weak_storage = storage(&path);
    vault.payload.entries.push(entry("kept"));
    weak_storage.save(&vault).unwrap();
    weak_storage.save(&vault).unwrap();
    assert!(weak_storage.backup_path(1).exists());
    drop(weak_storage);

    let stronger = KdfParams::new(KdfParams::MINIMUM.m_cost_kib(), 3, 1).unwrap();
    let strong_storage = storage(&path).with_kdf_params(stronger);
    let upgraded = strong_storage.open(PASSWORD).unwrap();

    assert_eq!(upgraded.kdf_params(), stronger);
    assert_ne!(upgraded.salt(), vault.salt(), "a fresh salt");
    assert_eq!(labels(&upgraded), ["kept"]);
    let on_disk = VaultEnvelope::from_bytes(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(on_disk.header().kdf_params, stronger);
    for n in 1..=MAX_BACKUPS {
      assert!(
        !strong_storage.backup_path(n).exists(),
        "weakly-keyed backup {n} must be gone"
      );
    }
    assert!(temp_files(dir.path()).is_empty());
    drop(strong_storage);

    // Opening with the weaker policy must not downgrade it again.
    let reopened = storage(&path).open(PASSWORD).unwrap();
    assert_eq!(reopened.kdf_params(), stronger);
  }

  #[test]
  fn verify_password_and_key_matches_compare_against_the_vault_key() {
    let (_dir, path) = temp_vault();
    let vault = storage(&path).create(PASSWORD).unwrap();

    assert!(vault.verify_password(PASSWORD).unwrap());
    assert!(
      !vault
        .verify_password(b"correct horse battery stapler")
        .unwrap()
    );

    let candidate = derive_key(PASSWORD, &vault.salt(), &vault.kdf_params()).unwrap();
    assert!(vault.key_matches(&candidate));
    let other_salt = generate_salt().unwrap();
    let wrong = derive_key(PASSWORD, &other_salt, &vault.kdf_params()).unwrap();
    assert!(!vault.key_matches(&wrong));
  }

  // ---- backups -----------------------------------------------------------

  #[test]
  fn save_rotates_backups_and_prunes_beyond_max() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    let unlocked = storage.create(PASSWORD).unwrap();

    let mut versions = vec![fs::read(&path).unwrap()];
    for _ in 0..MAX_BACKUPS + 2 {
      storage.save(&unlocked).unwrap();
      versions.push(fs::read(&path).unwrap());
    }

    // `.bak.n` is the vault as it was n saves ago.
    for n in 1..=MAX_BACKUPS {
      assert_eq!(
        fs::read(storage.backup_path(n)).unwrap(),
        versions[versions.len() - 1 - n],
        "backup slot {n}"
      );
    }
    assert!(
      !storage.backup_path(MAX_BACKUPS + 1).exists(),
      "backups beyond MAX_BACKUPS should have been pruned"
    );
  }

  #[test]
  fn save_without_rotation_leaves_backups_alone() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    let mut vault = storage.create(PASSWORD).unwrap();
    storage.save(&vault).unwrap();
    let backup_before = fs::read(storage.backup_path(1)).unwrap();

    vault.payload.entries.push(entry("counter bump"));
    storage.save_without_rotation(&vault).unwrap();

    assert_eq!(fs::read(storage.backup_path(1)).unwrap(), backup_before);
    assert!(!storage.backup_path(2).exists());
    assert_eq!(labels(&storage.open(PASSWORD).unwrap()), ["counter bump"]);
  }

  #[test]
  fn backups_work_for_non_utf8_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(OsStr::from_bytes(b"v\xffault"));
    let storage = storage(&path);
    let vault = storage.create(PASSWORD).unwrap();

    storage.save(&vault).unwrap();

    assert!(
      dir
        .path()
        .join(OsStr::from_bytes(b"v\xffault.bak.1"))
        .exists()
    );
    assert!(!dir.path().join("vault.bak.1").exists());
  }

  /// Finds the orphaned copy of backup slot `n`, wherever
  /// `orphan_backups_left_by_a_missing_vault` put it (the exact name
  /// depends on the current time, which the test does not control).
  fn find_orphaned_backup(dir: &Path, n: usize) -> PathBuf {
    let suffix = format!(".bak.{n}");
    fs::read_dir(dir)
      .unwrap()
      .map(|entry| entry.unwrap().path())
      .find(|path| {
        let name = path.file_name().unwrap().to_string_lossy();
        name.starts_with("vault.orphaned-") && name.ends_with(&suffix)
      })
      .unwrap_or_else(|| panic!("no orphaned backup for slot {n} in {dir:?}"))
  }

  #[test]
  fn create_moves_pre_existing_backups_out_of_the_rotation_ring() {
    let (dir, path) = temp_vault();
    // Backups left behind by a vault that went missing some other way
    // than a save (deleted by mistake, a sync tool, fs repair).
    let mut orphaned_contents = Vec::new();
    for n in 1..=MAX_BACKUPS {
      let bytes = format!("old backup {n}").into_bytes();
      fs::write(path.with_added_extension(format!("bak.{n}")), &bytes).unwrap();
      orphaned_contents.push(bytes);
    }

    let storage = storage(&path);
    let vault = storage.create(PASSWORD).unwrap();

    // The rotation-ring names are free again...
    for n in 1..=MAX_BACKUPS {
      assert!(!storage.backup_path(n).exists(), "slot {n} not cleared");
    }
    // ...and every old backup survives, byte-for-byte, under an orphaned
    // name rotation never touches.
    for (n, expected) in (1..=MAX_BACKUPS).zip(&orphaned_contents) {
      assert_eq!(
        &fs::read(find_orphaned_backup(dir.path(), n)).unwrap(),
        expected
      );
    }

    // Several rotating saves must not disturb the orphaned files.
    for _ in 0..MAX_BACKUPS + 2 {
      storage.save(&vault).unwrap();
    }
    for (n, expected) in (1..=MAX_BACKUPS).zip(&orphaned_contents) {
      assert_eq!(
        &fs::read(find_orphaned_backup(dir.path(), n)).unwrap(),
        expected
      );
    }
    // The rotation ring itself keeps working on the new vault's own
    // history, unrelated to the orphaned files.
    for n in 1..=MAX_BACKUPS {
      assert!(
        storage.backup_path(n).exists(),
        "slot {n} rotating normally"
      );
    }
  }

  #[test]
  fn create_leaves_backups_alone_when_a_vault_already_exists() {
    let (_dir, path) = temp_vault();
    let storage = storage(&path);
    let vault = storage.create(PASSWORD).unwrap();
    storage.save(&vault).unwrap();
    let backup_before = fs::read(storage.backup_path(1)).unwrap();

    let result = storage.create(b"a different password");

    assert!(matches!(result, Err(StorageError::AlreadyExists(_))));
    assert_eq!(fs::read(storage.backup_path(1)).unwrap(), backup_before);
  }

  // ---- crash safety ------------------------------------------------------
  //
  // The kill-mid-write test lives in `tests/crash_consistency.rs`: it
  // forks, so it needs a test binary of its own.

  #[test]
  fn a_failed_rotation_leaves_the_vault_intact() {
    let (dir, path) = temp_vault();
    let storage = storage(&path);
    let mut vault = storage.create(PASSWORD).unwrap();
    let before = fs::read(&path).unwrap();
    // A non-empty directory where the oldest backup goes cannot be
    // removed as a file, so rotation fails after the new contents were
    // staged but before they were committed.
    let oldest = storage.backup_path(MAX_BACKUPS);
    fs::create_dir(&oldest).unwrap();
    fs::write(oldest.join("keep"), b"x").unwrap();

    vault.payload.entries.push(entry("never saved"));
    assert!(matches!(storage.save(&vault), Err(StorageError::Io { .. })));

    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(temp_files(dir.path()).is_empty(), "staged file discarded");
    assert!(storage.open(PASSWORD).unwrap().payload.entries.is_empty());
  }

  // ---- locking and file-system hygiene -----------------------------------

  #[test]
  fn a_second_handle_on_the_same_vault_is_refused_until_the_first_drops() {
    let (_dir, path) = temp_vault();
    let first = storage(&path);

    assert!(matches!(
      VaultStorage::new(path.clone()),
      Err(StorageError::InUse(_))
    ));

    drop(first);
    assert!(VaultStorage::new(path).is_ok());
  }

  #[test]
  fn stale_temp_files_are_removed_when_the_vault_is_opened() {
    let (dir, path) = temp_vault();
    fs::write(dir.path().join(".vault-Ab12Cd"), b"half a vault").unwrap();
    fs::write(dir.path().join("notes.txt"), b"unrelated").unwrap();

    let _storage = storage(&path);

    assert!(temp_files(dir.path()).is_empty());
    assert!(dir.path().join("notes.txt").exists());
  }

  #[test]
  fn vault_dir_and_files_are_owner_only() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("arsu").join("vault");
    let storage = storage(&path);
    let vault = storage.create(PASSWORD).unwrap();
    storage.save(&vault).unwrap();

    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(path.parent().unwrap()), 0o700, "vault directory");
    assert_eq!(mode(&path), 0o600, "vault file");
    assert_eq!(mode(&storage.backup_path(1)), 0o600, "backup");
    assert_eq!(mode(&path.with_added_extension("lock")), 0o600, "lock file");
  }

  #[test]
  fn a_path_without_a_directory_is_rejected() {
    assert!(matches!(
      VaultStorage::new(PathBuf::from("vault")),
      Err(StorageError::Io { .. })
    ));
  }
}
