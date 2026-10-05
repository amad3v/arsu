//! Biometric unlock, on the vault's side: the vault key sealed by a
//! [`KeySealer`], kept next to the vault as `vault.biometric`.
//!
//! The file holds only the sealed key, worthless without the sealer's
//! hardware-held key. It is deleted whenever it can no longer open the
//! vault: the sealer's key is gone (the phone's biometrics changed), or
//! the vault's key changed (a new vault, a key-derivation upgrade). The
//! user then unlocks with the password and turns biometric unlock on
//! again.

use std::{
  fs, io,
  path::{Path, PathBuf},
};

use base64::{Engine, prelude::BASE64_STANDARD};
use crypto::MasterKey;
use serde::{Deserialize, Serialize};
use storage::StorageError;
use uuid::Uuid;

use super::{
  AppState,
  entries::{entry_qr, find_entry},
};
use crate::{
  biometric::{KeySealer, SealError, Sealed, UnsealFor},
  dto::{SecretQrSvg, SecretString},
  error::AppError,
};

/// The sealed key's file format version.
const SLOT_VERSION: u32 = 1;

/// The sealed vault key, as stored.
#[derive(Serialize, Deserialize)]
struct SlotFile {
  version: u32,
  iv: String,
  ciphertext: String,
}

impl AppState {
  /// Where the sealed vault key lives: `vault.biometric`, next to the
  /// vault.
  fn biometric_slot_path(&self) -> PathBuf {
    self.paths.vault().with_added_extension("biometric")
  }

  /// Whether biometric unlock is turned on (a sealed key is stored).
  ///
  /// # Errors
  ///
  /// [`AppError::FileRead`] if that cannot be told.
  pub fn biometric_enabled(&self) -> Result<bool, AppError> {
    let path = self.biometric_slot_path();
    path
      .try_exists()
      .map_err(|source| AppError::FileRead { path, source })
  }

  /// Turns biometric unlock on: checks `master_password` against the
  /// unlocked vault, then seals the vault key with `sealer`, which asks
  /// for the user's biometric, and stores it. Replaces any earlier seal.
  ///
  /// The password is asked again, as for showing a QR code: whoever
  /// holds an unlocked phone should not be able to add their own way in.
  ///
  /// # Errors
  ///
  /// [`AppError::Locked`], [`AppError::WrongPassword`],
  /// [`AppError::Biometric`] if the sealer refuses, or
  /// [`AppError::FileWrite`].
  pub fn enable_biometric_unlock(
    &self,
    master_password: &SecretString,
    sealer: &impl KeySealer,
  ) -> Result<(), AppError> {
    let (salt, kdf_params) = {
      let mut session = self.session();
      session.record_activity();
      let vault = session.unlocked()?;
      (vault.salt(), vault.kdf_params())
    };
    // The slow part, with no lock held.
    let key = crypto::derive_key(master_password.expose().as_bytes(), &salt, &kdf_params)?;
    if !self.session().unlocked()?.key_matches(&key) {
      return Err(AppError::WrongPassword);
    }

    let wrapped = sealer.seal(key.expose_for_sealing())?;
    let slot = SlotFile {
      version: SLOT_VERSION,
      iv: BASE64_STANDARD.encode(&wrapped.iv),
      ciphertext: BASE64_STANDARD.encode(&wrapped.ciphertext),
    };
    let path = self.biometric_slot_path();
    let bytes = serde_json::to_vec(&slot).map_err(|e| AppError::FileWrite {
      path: path.clone(),
      source: io::Error::other(e),
    })?;
    storage::atomic_write(&path, &bytes).map_err(|source| {
      // Nothing points at the new seal: drop it with its key.
      let _ = sealer.forget();
      AppError::FileWrite {
        path: path.clone(),
        source,
      }
    })?;
    self.record_activity();
    Ok(())
  }

  /// Unlocks the vault with the sealed key, which `sealer` releases once
  /// the user passes its biometric check.
  ///
  /// # Errors
  ///
  /// [`AppError::BiometricNotEnabled`] if no key is sealed;
  /// [`AppError::Biometric`] with [`SealError::Invalidated`] if the sealed
  /// key can no longer open the vault (it is deleted, so the user unlocks
  /// with the password and turns biometric unlock on again); or the
  /// sealer's refusal, or an error reading the vault.
  pub fn unlock_with_biometric(&self, sealer: &impl KeySealer) -> Result<(), AppError> {
    let key = self.unseal_vault_key(sealer, UnsealFor::Unlock)?;
    match self.storage()?.open_with_key(key) {
      Ok(vault) => {
        self.start_session(vault);
        Ok(())
      }
      // Not this vault's key any more: it was re-keyed, or replaced.
      Err(StorageError::Crypto(crypto::CryptoError::Decrypt)) => Err(self.drop_stale_slot(sealer)),
      Err(error) => Err(error.into()),
    }
  }

  /// The entry as a QR code, once the user passes the biometric check:
  /// with biometric unlock on, the fingerprint or face stands in for the
  /// master password that [`Self::export_entry_qr`] asks for. Both prove
  /// that the vault's owner, not just whoever holds the unlocked phone,
  /// is asking.
  ///
  /// # Errors
  ///
  /// [`AppError::Locked`], [`AppError::EntryNotFound`], or as
  /// [`Self::unlock_with_biometric`].
  pub fn export_entry_qr_with_biometric(
    &self,
    id: Uuid,
    sealer: &impl KeySealer,
  ) -> Result<SecretQrSvg, AppError> {
    {
      let mut session = self.session();
      session.record_activity();
      // Fail fast, before the prompt.
      find_entry(&session.unlocked()?.payload, id)?;
    }

    let key = self.unseal_vault_key(sealer, UnsealFor::ShowSecret)?;
    let session = self.session();
    let vault = session.unlocked()?;
    if !vault.key_matches(&key) {
      drop(session);
      return Err(self.drop_stale_slot(sealer));
    }
    entry_qr(&vault.payload, id)
  }

  /// The vault key, unsealed by `sealer` once the user passes its
  /// biometric check. A sealed key that is gone or garbled is deleted.
  fn unseal_vault_key(
    &self,
    sealer: &impl KeySealer,
    purpose: UnsealFor,
  ) -> Result<MasterKey, AppError> {
    let path = self.biometric_slot_path();
    let Some(wrapped) = read_slot(&path)? else {
      return Err(AppError::BiometricNotEnabled);
    };
    let secret = match sealer.open(&wrapped, purpose) {
      Err(SealError::Invalidated) => return Err(self.drop_stale_slot(sealer)),
      result => result?,
    };
    MasterKey::from_sealed(&secret).ok_or_else(|| self.drop_stale_slot(sealer))
  }

  /// Turns biometric unlock off: deletes the sealed key and the sealer's
  /// key.
  ///
  /// # Errors
  ///
  /// [`AppError::FileWrite`] if the file cannot be deleted, or
  /// [`AppError::Biometric`] if the sealer cannot delete its key.
  pub fn disable_biometric_unlock(&self, sealer: &impl KeySealer) -> Result<(), AppError> {
    self.remove_biometric_slot()?;
    sealer.forget()?;
    Ok(())
  }

  /// Deletes the sealed key, if there is one. A new vault does this: a
  /// key sealed for an earlier vault cannot open it.
  pub(super) fn remove_biometric_slot(&self) -> Result<(), AppError> {
    let path = self.biometric_slot_path();
    match fs::remove_file(&path) {
      Err(source) if source.kind() != io::ErrorKind::NotFound => {
        Err(AppError::FileWrite { path, source })
      }
      _ => Ok(()),
    }
  }

  /// The sealed key can no longer open the vault: deletes it, best effort,
  /// and returns the error that says biometric unlock is off.
  fn drop_stale_slot(&self, sealer: &impl KeySealer) -> AppError {
    let _ = self.remove_biometric_slot();
    let _ = sealer.forget();
    AppError::Biometric(SealError::Invalidated)
  }
}

/// The sealed key at `path`, or `None` if there is none. A file that
/// cannot be parsed counts as none: it seals nothing usable.
fn read_slot(path: &Path) -> Result<Option<Sealed>, AppError> {
  let bytes = match fs::read(path) {
    Ok(bytes) => bytes,
    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
    Err(source) => {
      return Err(AppError::FileRead {
        path: path.to_owned(),
        source,
      });
    }
  };
  let Ok(slot) = serde_json::from_slice::<SlotFile>(&bytes) else {
    return Ok(None);
  };
  if slot.version != SLOT_VERSION {
    return Ok(None);
  }
  match (
    BASE64_STANDARD.decode(slot.iv),
    BASE64_STANDARD.decode(slot.ciphertext),
  ) {
    (Ok(iv), Ok(ciphertext)) => Ok(Some(Sealed { iv, ciphertext })),
    _ => Ok(None),
  }
}

#[cfg(test)]
mod tests {
  use std::cell::{Cell, RefCell};

  use zeroize::Zeroizing;

  use super::*;
  use crate::{error::AppErrorKind, state::fixture::*};

  /// Seals by XOR with a byte, standing in for the phone's hardware: the
  /// user's answer to the prompt is `answer`.
  struct FakeSealer {
    key: Cell<Option<u8>>,
    answer: RefCell<Result<(), SealError>>,
  }

  impl FakeSealer {
    fn new() -> Self {
      Self {
        key: Cell::new(None),
        answer: RefCell::new(Ok(())),
      }
    }

    fn answer(&self, answer: Result<(), SealError>) {
      *self.answer.borrow_mut() = answer;
    }

    fn ask(&self) -> Result<(), SealError> {
      self.answer.replace(Ok(()))
    }
  }

  impl KeySealer for FakeSealer {
    fn seal(&self, secret: &[u8]) -> Result<Sealed, SealError> {
      self.ask()?;
      self.key.set(Some(0x5a));
      Ok(Sealed {
        iv: vec![1, 2, 3],
        ciphertext: secret.iter().map(|byte| byte ^ 0x5a).collect(),
      })
    }

    fn open(&self, sealed: &Sealed, _purpose: UnsealFor) -> Result<Zeroizing<Vec<u8>>, SealError> {
      let key = self.key.get().ok_or(SealError::Invalidated)?;
      self.ask()?;
      Ok(Zeroizing::new(
        sealed.ciphertext.iter().map(|byte| byte ^ key).collect(),
      ))
    }

    fn forget(&self) -> Result<(), SealError> {
      self.key.set(None);
      Ok(())
    }
  }

  fn kind(result: Result<(), AppError>) -> AppErrorKind {
    result.unwrap_err().kind()
  }

  #[test]
  fn a_fingerprint_unlocks_once_turned_on_with_the_password() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    assert!(!state.biometric_enabled().unwrap());

    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();
    assert!(state.biometric_enabled().unwrap());
    state.lock_vault();

    state.unlock_with_biometric(&sealer).unwrap();
    assert!(state.is_unlocked());
  }

  #[test]
  fn turning_it_on_takes_the_master_password_and_an_unlocked_vault() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();

    assert_eq!(
      kind(state.enable_biometric_unlock(&secret("not the password"), &sealer)),
      AppErrorKind::WrongPassword
    );
    state.lock_vault();
    assert_eq!(
      kind(state.enable_biometric_unlock(&secret(PASSWORD), &sealer)),
      AppErrorKind::Locked
    );
    assert!(!state.biometric_enabled().unwrap());
  }

  #[test]
  fn a_cancelled_prompt_changes_nothing() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();

    sealer.answer(Err(SealError::Cancelled));
    assert_eq!(
      kind(state.enable_biometric_unlock(&secret(PASSWORD), &sealer)),
      AppErrorKind::BiometricCancelled
    );
    assert!(!state.biometric_enabled().unwrap());

    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();
    state.lock_vault();
    sealer.answer(Err(SealError::Cancelled));
    assert_eq!(
      kind(state.unlock_with_biometric(&sealer)),
      AppErrorKind::BiometricCancelled
    );
    assert!(!state.is_unlocked());
    assert!(state.biometric_enabled().unwrap(), "still on for next time");
  }

  #[test]
  fn changed_biometrics_turn_it_off() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();
    state.lock_vault();

    // The phone deletes the key when a fingerprint is added.
    sealer.key.set(None);

    assert_eq!(
      kind(state.unlock_with_biometric(&sealer)),
      AppErrorKind::BiometricInvalidated
    );
    assert!(!state.biometric_enabled().unwrap());
    state.unlock_vault(&secret(PASSWORD)).unwrap();
  }

  #[test]
  fn a_key_sealed_for_another_vault_turns_it_off() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();
    state.lock_vault();

    // The slot now seals some other key.
    let slot = state.biometric_slot_path();
    let mut file: SlotFile = serde_json::from_slice(&fs::read(&slot).unwrap()).unwrap();
    file.ciphertext = BASE64_STANDARD.encode([0x5a ^ 7; crypto::KEY_LEN]);
    fs::write(&slot, serde_json::to_vec(&file).unwrap()).unwrap();

    assert_eq!(
      kind(state.unlock_with_biometric(&sealer)),
      AppErrorKind::BiometricInvalidated
    );
    assert!(!state.biometric_enabled().unwrap());
    assert!(
      sealer.key.get().is_none(),
      "the sealer's key is dropped too"
    );
  }

  #[test]
  fn turning_it_off_deletes_both_keys() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();

    state.disable_biometric_unlock(&sealer).unwrap();

    assert!(!state.biometric_enabled().unwrap());
    assert!(sealer.key.get().is_none());
    state.lock_vault();
    assert_eq!(
      kind(state.unlock_with_biometric(&sealer)),
      AppErrorKind::BiometricNotEnabled
    );
  }

  const ENTRY_URI: &str = "otpauth://totp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP";

  #[test]
  fn a_fingerprint_shows_a_qr_code_in_place_of_the_password() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    let id = state.add_entry_from_uri(&secret(ENTRY_URI)).unwrap();

    assert_eq!(
      state
        .export_entry_qr_with_biometric(id, &sealer)
        .unwrap_err()
        .kind(),
      AppErrorKind::BiometricNotEnabled
    );

    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();
    let svg = state.export_entry_qr_with_biometric(id, &sealer).unwrap();
    assert!(svg.as_str().contains("<svg"));

    sealer.answer(Err(SealError::Cancelled));
    assert_eq!(
      state
        .export_entry_qr_with_biometric(id, &sealer)
        .unwrap_err()
        .kind(),
      AppErrorKind::BiometricCancelled
    );
    assert!(
      state.biometric_enabled().unwrap(),
      "a cancel changes nothing"
    );

    state.lock_vault();
    assert_eq!(
      state
        .export_entry_qr_with_biometric(id, &sealer)
        .unwrap_err()
        .kind(),
      AppErrorKind::Locked
    );
  }

  #[test]
  fn a_qr_code_needs_the_open_vaults_own_key() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    let id = state.add_entry_from_uri(&secret(ENTRY_URI)).unwrap();
    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();

    // The slot now seals some other key.
    let slot = state.biometric_slot_path();
    let mut file: SlotFile = serde_json::from_slice(&fs::read(&slot).unwrap()).unwrap();
    file.ciphertext = BASE64_STANDARD.encode([0x5a ^ 7; crypto::KEY_LEN]);
    fs::write(&slot, serde_json::to_vec(&file).unwrap()).unwrap();

    assert_eq!(
      state
        .export_entry_qr_with_biometric(id, &sealer)
        .unwrap_err()
        .kind(),
      AppErrorKind::BiometricInvalidated
    );
    assert!(!state.biometric_enabled().unwrap());
  }

  #[test]
  fn the_file_holds_no_key_in_the_clear() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let sealer = FakeSealer::new();
    state
      .enable_biometric_unlock(&secret(PASSWORD), &sealer)
      .unwrap();

    let stored = fs::read_to_string(state.biometric_slot_path()).unwrap();
    let vault = state.session();
    let unlocked = vault.unlocked().unwrap();
    let key = crypto::derive_key(
      PASSWORD.as_bytes(),
      &unlocked.salt(),
      &unlocked.kdf_params(),
    )
    .unwrap();
    assert!(!stored.contains(&BASE64_STANDARD.encode(key.expose_for_sealing())));
  }
}
