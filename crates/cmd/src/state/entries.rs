//! Reading, adding and removing vault entries, and generating codes.

use otp::{OtpError, Totp};
use qr::{ParsedAccount, build_otpauth_uri, encode_qr_svg, parse_manual_entry, parse_otpauth_uri};
use storage::VaultStorage;
use uuid::Uuid;
use vault_core::{Entry, OtpConfig, VaultPayload};

use super::AppState;
use crate::{
  clock::unix_now,
  dto::{CodeResponse, EntrySummary, ManualEntryInput, SecretQrSvg, SecretString},
  error::AppError,
};

impl AppState {
  /// The active entries, in the order they were added.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::Locked`] if the vault is locked.
  pub fn list_entries(&self) -> Result<Vec<EntrySummary>, AppError> {
    Ok(
      self
        .session()
        .unlocked()?
        .payload
        .active_entries()
        .map(EntrySummary::from)
        .collect(),
    )
  }

  /// The entry's code at `unix_time` (seconds).
  ///
  /// For TOTP this only reads. An HOTP code is used up once shown
  /// (RFC 4226), so its counter is advanced and saved before the code is
  /// returned: a crash can never leave the counter on disk behind a code
  /// the user has already seen. That save does not rotate the backups
  /// ([`VaultStorage::save_without_rotation`]).
  ///
  /// Not user activity: the entry list refreshes codes on its own.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::Locked`], [`AppError::EntryNotFound`],
  /// [`OtpError::CounterExhausted`] for an HOTP counter that cannot
  /// advance, or an error if saving the advanced counter fails — in
  /// which case no code is returned and the counter is unchanged.
  pub fn current_code(&self, id: Uuid, unix_time: u64) -> Result<CodeResponse, AppError> {
    let storage = self.storage()?;
    let mut session = self.session();
    match &find_entry(&session.unlocked()?.payload, id)?.otp {
      OtpConfig::Totp {
        secret,
        algorithm,
        digits,
        period,
      } => {
        let totp = Totp::new(secret.expose_secret(), *algorithm, *digits, *period)?;
        Ok(CodeResponse::totp(&totp, unix_time))
      }
      OtpConfig::Hotp {
        secret,
        algorithm,
        digits,
        counter,
      } => {
        let code = otp::hotp(secret.expose_secret(), *counter, *algorithm, *digits)?;
        let advanced = OtpConfig::Hotp {
          secret: secret.clone(),
          algorithm: *algorithm,
          digits: *digits,
          counter: counter.checked_add(1).ok_or(OtpError::CounterExhausted)?,
        };
        session.commit(&storage, VaultStorage::save_without_rotation, |payload| {
          let entry = find_entry_mut(payload, id)?;
          entry.otp = advanced;
          entry.updated_at = unix_time;
          Ok(())
        })?;
        Ok(CodeResponse::hotp(code))
      }
    }
  }

  /// Adds the account an `otpauth://` URI describes (pasted, or read
  /// from a QR code by the frontend). Returns the new entry's id.
  ///
  /// # Errors
  ///
  /// Returns an [`AppError::Account`] error if the URI is not a valid
  /// account, [`AppError::Locked`], or an error if the vault cannot be
  /// saved.
  pub fn add_entry_from_uri(&self, uri: &SecretString) -> Result<Uuid, AppError> {
    self.add_entry(parse_otpauth_uri(uri.expose())?)
  }

  /// Adds a manually entered account. Returns the new entry's id.
  ///
  /// # Errors
  ///
  /// As for [`Self::add_entry_from_uri`].
  pub fn add_entry_manual(&self, input: ManualEntryInput) -> Result<Uuid, AppError> {
    self.add_entry(parse_manual_entry(input.into())?)
  }

  fn add_entry(&self, account: ParsedAccount) -> Result<Uuid, AppError> {
    let entry = new_entry(account, unix_now()?);
    let id = entry.id;
    let storage = self.storage()?;
    let mut session = self.session();
    session.record_activity();
    session.commit(&storage, VaultStorage::save, |payload| {
      payload.entries.push(entry);
      Ok(())
    })?;
    Ok(id)
  }

  /// Deletes an entry. It is tombstoned rather than removed (see
  /// `vault_core::Entry::deleted_at`), which from the frontend's side is
  /// the same thing.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::Locked`], [`AppError::EntryNotFound`], or an
  /// error if the vault cannot be saved (the entry then stays).
  pub fn delete_entry(&self, id: Uuid) -> Result<(), AppError> {
    let now = unix_now()?;
    let storage = self.storage()?;
    let mut session = self.session();
    session.record_activity();
    session.commit(&storage, VaultStorage::save, |payload| {
      let entry = find_entry_mut(payload, id)?;
      entry.deleted_at = Some(now);
      entry.updated_at = now;
      Ok(())
    })
  }

  /// Renders one entry as a QR code (SVG), to move it to another app.
  ///
  /// The QR code encodes the entry's secret, so this is gated on the
  /// master password: `master_password` is run through the KDF with the
  /// vault's salt and parameters, without holding any lock, and the
  /// result compared with the vault's key in constant time. A script in
  /// the `WebView` cannot harvest seeds without it, and each attempt costs
  /// a full key derivation. There is deliberately no way to export the
  /// whole vault as QR codes.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::WrongPassword`], [`AppError::Locked`],
  /// [`AppError::EntryNotFound`], or [`qr::QrError::QrEncode`] if the
  /// entry is too long for a QR code.
  pub fn export_entry_qr(
    &self,
    id: Uuid,
    master_password: &SecretString,
  ) -> Result<SecretQrSvg, AppError> {
    let (salt, kdf_params) = {
      let mut session = self.session();
      session.record_activity();
      let vault = session.unlocked()?;
      // Fail fast, before the slow key derivation.
      find_entry(&vault.payload, id)?;
      (vault.salt(), vault.kdf_params())
    };

    let candidate = crypto::derive_key(master_password.expose().as_bytes(), &salt, &kdf_params)?;

    let session = self.session();
    let vault = session.unlocked()?;
    if !vault.key_matches(&candidate) {
      return Err(AppError::WrongPassword);
    }
    let entry = find_entry(&vault.payload, id)?;
    let uri = build_otpauth_uri(entry.issuer.as_deref(), &entry.account_label, &entry.otp);
    Ok(SecretQrSvg::from(encode_qr_svg(&uri)?))
  }
}

/// The vault entry for a newly added account: the one place entries
/// are created, whether the account was typed in, scanned or imported.
pub(super) fn new_entry(account: ParsedAccount, now: u64) -> Entry {
  let (issuer, account_label, otp) = account.into_parts();
  Entry {
    id: Uuid::now_v7(),
    issuer,
    account_label,
    otp,
    icon: None,
    tags: Vec::new(),
    notes: None,
    created_at: now,
    updated_at: now,
    deleted_at: None,
  }
}

fn find_entry(payload: &VaultPayload, id: Uuid) -> Result<&Entry, AppError> {
  payload
    .active_entries()
    .find(|entry| entry.id == id)
    .ok_or(AppError::EntryNotFound)
}

fn find_entry_mut(payload: &mut VaultPayload, id: Uuid) -> Result<&mut Entry, AppError> {
  payload
    .entries
    .iter_mut()
    .find(|entry| entry.id == id && entry.is_active())
    .ok_or(AppError::EntryNotFound)
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::*;
  use crate::{error::AppErrorKind, state::fixture::*};

  const GOOGLE_EXAMPLE: &str =
    "otpauth://totp/Example:alice@google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example";

  /// RFC 4226 Appendix D: "12345678901234567890" in base32.
  const RFC4226_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

  fn hotp_input(counter: u64) -> ManualEntryInput {
    serde_json::from_value(json!({
      "issuer": "RFC",
      "accountLabel": "4226",
      "secretBase32": RFC4226_SECRET,
      "algorithm": "sha1",
      "digits": 6,
      "type": "hotp",
      "counter": counter,
    }))
    .unwrap()
  }

  #[test]
  fn create_add_lock_unlock_and_generate_a_code() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_from_uri(&secret(GOOGLE_EXAMPLE)).unwrap();

    state.lock_vault();
    state.unlock_vault(&secret(PASSWORD)).unwrap();

    let entries = state.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, id.to_string());
    assert_eq!(entries[0].issuer.as_deref(), Some("Example"));
    assert_eq!(entries[0].account_label, "alice@google.com");
    assert_eq!(entries[0].period, Some(30));

    let code = state.current_code(id, 1_000_000_000).unwrap();
    let totp = Totp::new(b"Hello!\xDE\xAD\xBE\xEF", otp::Algorithm::Sha1, 6, 30).unwrap();
    assert_eq!(code.code, totp.generate(1_000_000_000));
    assert_eq!(code.next_code, Some(totp.generate(1_000_000_030)));
    assert_eq!(code.expires_in_seconds, Some(20));
  }

  #[test]
  fn entries_survive_a_restart() {
    let fixture = Fixture::unlocked();
    let id = fixture
      .state
      .add_entry_from_uri(&secret(GOOGLE_EXAMPLE))
      .unwrap();
    let before = fixture.state.current_code(id, 59).unwrap().code;

    let restarted = fixture.restart();
    restarted.state.unlock_vault(&secret(PASSWORD)).unwrap();
    assert_eq!(restarted.state.current_code(id, 59).unwrap().code, before);
  }

  #[test]
  fn hotp_codes_advance_and_the_counter_survives_a_restart() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_manual(hotp_input(0)).unwrap();

    // RFC 4226 Appendix D, counters 0 and 1.
    assert_eq!(state.current_code(id, 0).unwrap().code, "755224");
    assert_eq!(state.current_code(id, 0).unwrap().code, "287082");

    state.lock_vault();
    state.unlock_vault(&secret(PASSWORD)).unwrap();
    let after_unlock = state.current_code(id, 0).unwrap();
    assert_eq!(after_unlock.code, "359152", "counter 2 was persisted");
    assert_eq!(after_unlock.expires_in_seconds, None);
    assert_eq!(after_unlock.next_code, None);
  }

  #[test]
  fn hotp_reveals_do_not_use_up_backups() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_manual(hotp_input(0)).unwrap();
    let backup = fixture.paths().vault().with_added_extension("bak.1");
    let backup_before = std::fs::read(&backup).unwrap();

    for _ in 0..5 {
      state.current_code(id, 0).unwrap();
    }

    assert_eq!(std::fs::read(&backup).unwrap(), backup_before);
  }

  #[test]
  fn an_exhausted_hotp_counter_is_an_error_not_a_crash() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_manual(hotp_input(u64::MAX - 1)).unwrap();

    state.current_code(id, 0).unwrap();
    let exhausted = state.current_code(id, 0).unwrap_err();

    assert_eq!(exhausted.kind(), AppErrorKind::CounterExhausted);
  }

  #[test]
  fn a_failed_counter_save_shows_no_code_and_keeps_the_counter() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_manual(hotp_input(0)).unwrap();

    fixture.make_vault_unwritable();
    assert_eq!(
      state.current_code(id, 0).unwrap_err().kind(),
      AppErrorKind::StorageIo
    );

    let session = state.session();
    let entry = find_entry(&session.unlocked().unwrap().payload, id).unwrap();
    assert!(matches!(entry.otp, OtpConfig::Hotp { counter: 0, .. }));
  }

  #[test]
  fn deleted_entries_disappear() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_from_uri(&secret(GOOGLE_EXAMPLE)).unwrap();

    state.delete_entry(id).unwrap();

    assert!(state.list_entries().unwrap().is_empty());
    for error in [
      state.current_code(id, 0).unwrap_err(),
      state.delete_entry(id).unwrap_err(),
      state.export_entry_qr(id, &secret(PASSWORD)).unwrap_err(),
    ] {
      assert_eq!(error.kind(), AppErrorKind::EntryNotFound);
    }
  }

  #[test]
  fn invalid_accounts_are_rejected_with_their_kind() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let cases = [
      ("https://example.com", AppErrorKind::NotOtpauth),
      (
        "otpauth://totp/a:b:c?secret=JBSWY3DPEHPK3PXP",
        AppErrorKind::LabelContainsColon,
      ),
      ("otpauth://totp/Example:alice", AppErrorKind::MissingSecret),
      (
        "otpauth://totp/Example:alice?secret=!!!",
        AppErrorKind::InvalidSecret,
      ),
      (
        "otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP&digits=9",
        AppErrorKind::InvalidDigits,
      ),
    ];
    for (uri, kind) in cases {
      assert_eq!(
        state.add_entry_from_uri(&secret(uri)).unwrap_err().kind(),
        kind,
        "{uri}"
      );
    }
    assert!(state.list_entries().unwrap().is_empty());
  }

  #[test]
  fn exporting_a_qr_code_needs_the_master_password() {
    let fixture = Fixture::unlocked();
    let state = &fixture.state;
    let id = state.add_entry_from_uri(&secret(GOOGLE_EXAMPLE)).unwrap();

    let wrong = state
      .export_entry_qr(id, &secret("not the password"))
      .unwrap_err();
    assert_eq!(wrong.kind(), AppErrorKind::WrongPassword);

    let svg = state.export_entry_qr(id, &secret(PASSWORD)).unwrap();
    assert!(svg.as_str().contains("<svg"), "{}", &svg.as_str()[..40]);

    state.lock_vault();
    assert_eq!(
      state
        .export_entry_qr(id, &secret(PASSWORD))
        .unwrap_err()
        .kind(),
      AppErrorKind::Locked
    );
  }
}
