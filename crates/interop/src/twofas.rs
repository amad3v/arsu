//! Import from 2FAS `.2fas` backup files.
//!
//! 2FAS has never published a format specification, so this module is
//! built from community reverse engineering, cross-checked against
//! `lib2fas` (github.com/robinvandernoord/lib2fas-rust), an independent
//! implementation. Two confidence levels apply:
//!
//! - **The encryption scheme is verified**: AES-256-GCM under a
//!   PBKDF2-HMAC-SHA256 key (10,000 iterations), stored as
//!   `base64(ciphertext‖tag):base64(salt):base64(nonce)`. It decrypts a
//!   published test vector byte for byte (see `tests`). The iteration
//!   count is fixed by the format, not read from the file, so a crafted
//!   file cannot raise the cost of deriving the key.
//! - **The per-service fields are lower confidence.** They follow what
//!   real backups are known to contain (a `services` array, each with
//!   `name`, `secret` and an `otp` object), without an official schema
//!   to check every field against. The parser is lenient: optional
//!   fields have sensible defaults, and a service that doesn't parse or
//!   validate is skipped with a reason while the rest still import.
//!
//! The decrypted payload is the same JSON the app writes for a plain
//! backup (the Android and iOS apps both serialize it with their
//! platform's standard JSON encoder), so it is parsed with `serde_json`.
//! `lib2fas` reads every file with a JSON5 parser, but that is a choice
//! of the library, which also exposes general JSON5 utilities, not a
//! property of the format; and the `json5` crate has no nesting limit.
//!
//! Only import is implemented: without a schema to build against,
//! producing a file the real 2FAS app accepts would be guesswork. Move
//! data out of this app with an Aegis export or a per-entry QR code
//! instead.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, de};
use serde_json::value::RawValue;
use sha2::Sha256;
use zeroize::Zeroizing;

use qr::{
  DEFAULT_ALGORITHM, DEFAULT_DIGITS, OtpKind, OtpParams, ParsedAccount, QrError, decode_secret,
  parse_algorithm, parse_otpauth_uri,
};

use crate::{
  ImportOutcome, InteropError, KnownAccounts, check_size,
  outcome::{SourceEntry, display_label, import_entries},
  secret::{KEY_LEN, NONCE_LEN, SecretText, TAG_LEN, open},
};

const PBKDF2_ITERATIONS: u32 = 10_000;

/// A backup file has one of the two service lists.
#[derive(Deserialize)]
struct BackupFile<'file> {
  /// The services of a plain backup; encrypted backups leave the list
  /// empty or out. Kept as raw JSON so each service is parsed on its
  /// own (see [`import_entries`]).
  #[serde(borrow)]
  services: Option<Vec<&'file RawValue>>,
  /// The services of an encrypted backup (see [`EncryptedField`]).
  #[serde(rename = "servicesEncrypted")]
  services_encrypted: Option<String>,
}

#[derive(Deserialize)]
struct RawService {
  name: Option<String>,
  secret: Option<SecretText>,
  otp: Option<RawOtp>,
}

#[derive(Deserialize, Default)]
struct RawOtp {
  issuer: Option<String>,
  account: Option<String>,
  /// `TOTP`, `HOTP` or `STEAM`; older backups leave it out.
  #[serde(rename = "tokenType")]
  token_type: Option<String>,
  algorithm: Option<String>,
  digits: Option<u8>,
  period: Option<u64>,
  counter: Option<u64>,
  /// A full `otpauth://` URI. Its secret is sometimes a redacted
  /// placeholder, so it is only used for a service without a `secret`.
  link: Option<SecretText>,
}

/// Imports the services in a `.2fas` backup file, leaving out those
/// `known` already holds.
///
/// `password` is required for an encrypted backup and ignored for a
/// plain one. A service that is malformed, fails validation, or has an
/// OTP type other than TOTP and HOTP (such as `STEAM`) is reported in
/// [`ImportOutcome::skipped`].
///
/// # Errors
///
/// - [`InteropError::FileTooLarge`] over [`crate::MAX_IMPORT_BYTES`];
/// - [`InteropError::UnrecognizedFormat`] if the file, or the decrypted
///   services, are not a 2FAS backup;
/// - [`InteropError::PasswordRequired`] for an encrypted backup without
///   a password;
/// - [`InteropError::Malformed`] if the encrypted services field is not
///   in the expected form;
/// - [`InteropError::WrongPasswordOrCorrupted`] if the services don't
///   decrypt with `password`.
pub fn import(
  file_bytes: &[u8],
  password: Option<&str>,
  known: &KnownAccounts,
) -> Result<ImportOutcome, InteropError> {
  check_size(file_bytes)?;
  let file: BackupFile<'_> =
    serde_json::from_slice(file_bytes).map_err(InteropError::UnrecognizedFormat)?;

  match (&file.services_encrypted, &file.services) {
    (Some(encrypted), _) => {
      let password = password.ok_or(InteropError::PasswordRequired)?;
      let plaintext = EncryptedField::parse(encrypted)?.decrypt(password)?;
      let services: Vec<&RawValue> =
        serde_json::from_slice(&plaintext).map_err(InteropError::UnrecognizedFormat)?;
      Ok(import_entries::<RawService>(&services, known))
    }
    (None, Some(services)) => Ok(import_entries::<RawService>(services, known)),
    (None, None) => Err(InteropError::UnrecognizedFormat(de::Error::missing_field(
      "services",
    ))),
  }
}

/// A field encrypted the way 2FAS encrypts `servicesEncrypted` (and the
/// `reference` password check):
/// `base64(ciphertext‖tag):base64(salt):base64(nonce)`, AES-256-GCM
/// under a PBKDF2-HMAC-SHA256 key.
struct EncryptedField {
  /// The ciphertext, and the plaintext once decrypted.
  buffer: Zeroizing<Vec<u8>>,
  tag: [u8; TAG_LEN],
  salt: Vec<u8>,
  nonce: [u8; NONCE_LEN],
}

impl EncryptedField {
  fn parse(field: &str) -> Result<Self, InteropError> {
    let malformed = || InteropError::Malformed("encrypted services");
    let decode = |part: &str| BASE64.decode(part).map_err(|_| malformed());

    let mut parts = field.split(':');
    let (Some(sealed), Some(salt), Some(nonce), None) =
      (parts.next(), parts.next(), parts.next(), parts.next())
    else {
      return Err(malformed());
    };

    let mut buffer = Zeroizing::new(decode(sealed)?);
    let tag_start = buffer.len().checked_sub(TAG_LEN).ok_or_else(malformed)?;
    let tag = buffer
      .split_off(tag_start)
      .try_into()
      .map_err(|_| malformed())?;
    Ok(Self {
      buffer,
      tag,
      salt: decode(salt)?,
      nonce: decode(nonce)?.try_into().map_err(|_| malformed())?,
    })
  }

  fn decrypt(mut self, password: &str) -> Result<Zeroizing<Vec<u8>>, InteropError> {
    let mut key = Zeroizing::new([0; KEY_LEN]);
    pbkdf2_hmac::<Sha256>(
      password.as_bytes(),
      &self.salt,
      PBKDF2_ITERATIONS,
      key.as_mut_slice(),
    );
    open(&key, &self.nonce, &mut self.buffer, &self.tag)
      .map_err(|_| InteropError::WrongPasswordOrCorrupted)?;
    Ok(self.buffer)
  }
}

impl SourceEntry for RawService {
  fn label(&self) -> Option<String> {
    let account = self.otp.as_ref().and_then(|otp| otp.account.as_deref());
    display_label(self.name.as_deref(), account)
  }

  fn into_account(self) -> Result<ParsedAccount, QrError> {
    let otp = self.otp.unwrap_or_default();

    // The top-level secret is authoritative; `otp.link` is only a
    // fallback, parsed by the same code as a scanned QR code.
    let Some(secret) = self.secret.filter(|secret| !secret.is_blank()) else {
      let link = otp.link.ok_or(QrError::MissingSecret)?;
      return parse_otpauth_uri(link.expose_secret());
    };

    let kind = match otp.token_type.as_deref() {
      Some(token_type) => token_type.parse()?,
      // Only older backups lack a token type; a counter then marks HOTP.
      None if otp.counter.is_some() => OtpKind::Hotp,
      None => OtpKind::Totp,
    };
    let params = OtpParams::for_kind(kind, otp.period, otp.counter)?;
    let algorithm = otp
      .algorithm
      .as_deref()
      .map_or(Ok(DEFAULT_ALGORITHM), parse_algorithm)?;
    // `name` is the title 2FAS shows (by default the issuer); a service
    // without an account name is named by it alone.
    let issuer = otp
      .issuer
      .as_deref()
      .filter(|issuer| !issuer.trim().is_empty())
      .or(self.name.as_deref());

    ParsedAccount::new(
      issuer,
      otp.account.as_deref().unwrap_or_default(),
      decode_secret(secret.expose_secret())?,
      algorithm,
      otp.digits.unwrap_or(DEFAULT_DIGITS),
      params,
    )
  }
}

#[cfg(test)]
mod tests {
  use otp::{Algorithm, OtpError};
  use serde_json::{Value, json};
  use vault_core::OtpConfig;

  use crate::{
    SkipReason,
    secret::{random_bytes, seal},
  };

  use super::*;

  /// Imports into an empty vault.
  fn import_fresh(
    file_bytes: &[u8],
    password: Option<&str>,
  ) -> Result<ImportOutcome, InteropError> {
    import(file_bytes, password, &KnownAccounts::default())
  }

  /// The reference string, password and expected plaintext from
  /// `lib2fas`'s own test suite (src/decrypt.rs `test_decrypt`), an
  /// implementation unrelated to this one. If this stops matching, the
  /// decryption itself is broken.
  const REFERENCE: &str = "NtGP2JfaqREfQcfe9gKllN3pd1+GkfJHrUSFEFZMRHMprFDJHB9Q7afMe00vNjTu4Jy4usCiu/RGU6rRN1gyJaTCqcOrM6xupVgX1AA/idp4faO2Gexcsg3mcqN8wWPJnc38YuP3V1Z0o6iu65ASsQnY7fm0fzB+CVzVgYTmzHbFU/un3xrmyUlxuzFL0PRlGgzoiDtfrG5CxPRvp1pO+WAoXNzgudABGcNUO/X0g6RKcetyaFwoko2190n+yr/tmV8KTDCfwxjm/BuGuTYXksT72AX5xB7scPtnrTQKOJnwYXxyEELdbyS0FKU/yajqOAlqx3zr59KCxlvreiYUfHRNiylTucI2mdh+VzK8RuI=:I90AMhbdg/2kEGuE/dkcp0O19XiavoPiy2HXsO/AAywDZMMLSPD1U+3pD8bckd4SlPwjmXvU+gjLd4m4a7WL7Uqb1fCawRQ74Lvp4WDjN5g7SmJhyLpvXjsRjLZj9q6HQOAsBeyxggGW7sAwCYTkpv7Vel77aSoHqx4fN7cDrLQ0/hOc+3WmgcxOXRTeex49F4gOsyWn5dunz0MhPIHnGFAYxMLyqXxMajalRqk8tJL7eCz4Umlm3tSrrI39aYpO1lGHPRoZMTx04aJuHJwrFVlqcAcJkGs2pzCQrXHtN8p7f/lMHrdpNEnc93wjWYs3XHPdiGdA8zPdU/AQRItFTQ==:p+HspW0nvUhneUmE";

  const EXPECTED_PLAINTEXT: &str = "tRViSsLKzd86Hprh4ceC2OP7xazn4rrt4xhfEUbOjxLX8Rc3mkISXE0lWbmnWfggogbBJhtYgpK6fMl1D6mtsy92R3HkdGfwuXbzLebqVFJsR7IZ2w58t938iymwG4824igYy1wi6n2WDpO1Q1P69zwJGs2F5a1qP4MyIiDSD7NCV2OvidXQCBnDlGfmz0f1BQySRkkt4ryiJeCjD2o4QsveJ9uDBUn8ELyOrESv5R5DMDkD4iAF8TXU7KyoJujd";

  /// Encrypts `plaintext` the way 2FAS does.
  fn encrypt_field(plaintext: &[u8], password: &str) -> String {
    let salt = random_bytes::<256>().unwrap();
    let nonce = random_bytes::<NONCE_LEN>().unwrap();
    let mut key = [0; KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &*salt, PBKDF2_ITERATIONS, &mut key);

    let mut sealed = plaintext.to_vec();
    let tag = seal(&key, &nonce, &mut sealed).unwrap();
    sealed.extend_from_slice(&tag);
    [
      BASE64.encode(sealed),
      BASE64.encode(*salt),
      BASE64.encode(*nonce),
    ]
    .join(":")
  }

  fn encrypted_backup(services: &str, password: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
      "services": [],
      "servicesEncrypted": encrypt_field(services.as_bytes(), password),
      "schemaVersion": 4,
    }))
    .unwrap()
  }

  fn import_plain(services: &Value) -> ImportOutcome {
    let file = serde_json::to_vec(&json!({ "services": services, "schemaVersion": 4 })).unwrap();
    import_fresh(&file, None).unwrap()
  }

  fn only_import(services: &Value) -> ParsedAccount {
    let mut outcome = import_plain(services);
    assert_eq!(outcome.imported.len(), 1, "skipped: {:?}", outcome.skipped);
    outcome.imported.remove(0)
  }

  // ---- encryption --------------------------------------------------------

  #[test]
  fn decrypts_the_lib2fas_reference_vector() {
    let plaintext = EncryptedField::parse(REFERENCE)
      .unwrap()
      .decrypt("test")
      .unwrap();
    assert_eq!(*plaintext, EXPECTED_PLAINTEXT.as_bytes());
  }

  #[test]
  fn rejects_wrong_password_on_the_reference_vector() {
    assert!(matches!(
      EncryptedField::parse(REFERENCE).unwrap().decrypt("wrong"),
      Err(InteropError::WrongPasswordOrCorrupted)
    ));
  }

  #[test]
  fn rejects_malformed_encrypted_fields() {
    for field in [
      "only-one-part",
      "a:b",
      "AAAA:AAAA:AAAA:AAAA",
      "!!:AAAA:AAAA",
      "AAAA:AAAA:AAAA",
    ] {
      assert!(
        matches!(
          EncryptedField::parse(field),
          Err(InteropError::Malformed(_))
        ),
        "{field}"
      );
    }
  }

  #[test]
  fn imports_an_encrypted_backup() {
    let services = r#"[{"name":"GitHub","secret":"JBSWY3DPEHPK3PXP","otp":{"account":"octocat","tokenType":"TOTP"}}]"#;
    let file = encrypted_backup(services, "hunter2");

    let outcome = import_fresh(&file, Some("hunter2")).unwrap();
    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.imported[0].account_label(), "octocat");

    assert!(matches!(
      import_fresh(&file, Some("hunter3")),
      Err(InteropError::WrongPasswordOrCorrupted)
    ));
    assert!(matches!(
      import_fresh(&file, None),
      Err(InteropError::PasswordRequired)
    ));
  }

  #[test]
  fn an_encrypted_backup_may_omit_the_services_key() {
    let services = r#"[{"name":"GitHub","secret":"JBSWY3DPEHPK3PXP"}]"#;
    let file = json!({ "servicesEncrypted": encrypt_field(services.as_bytes(), "pw") });

    let outcome = import_fresh(&serde_json::to_vec(&file).unwrap(), Some("pw")).unwrap();
    assert_eq!(outcome.imported.len(), 1);
  }

  #[test]
  fn a_deeply_nested_decrypted_payload_cannot_overflow_the_stack() {
    const DEPTH: usize = 100_000;
    let services = format!(
      r#"[{{"name":"Deep","secret":"JBSWY3DPEHPK3PXP","otp":{{"account":"a"}},"x":{open}{close}}},
          {{"name":"Deeper","secret":"JBSWY3DPEHPK3PXP","otp":{open}{close}}}]"#,
      open = "[".repeat(DEPTH),
      close = "]".repeat(DEPTH),
    );
    let file = encrypted_backup(&services, "pw");

    // Imports run on tokio's blocking pool, whose threads get 2 MiB.
    let outcome = std::thread::Builder::new()
      .stack_size(2 << 20)
      .spawn(move || import_fresh(&file, Some("pw")))
      .unwrap()
      .join()
      .unwrap()
      .unwrap();

    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.skipped[0].label, "Deeper");
    assert!(matches!(
      outcome.skipped[0].reason,
      SkipReason::Malformed(_)
    ));
  }

  // ---- services ----------------------------------------------------------

  #[test]
  fn imports_explicit_otp_fields() {
    let account = only_import(&json!([{
      "name": "GitHub",
      "secret": "JBSWY3DPEHPK3PXP",
      "otp": {
        "issuer": "GitHub", "account": "octocat", "tokenType": "TOTP",
        "algorithm": "SHA1", "digits": 6, "period": 30
      }
    }]));
    assert_eq!(account.issuer(), Some("GitHub"));
    assert_eq!(account.account_label(), "octocat");
  }

  /// Real 2FAS backups carry `"counter": 0` on TOTP services.
  #[test]
  fn a_totp_service_with_a_counter_stays_totp() {
    let account = only_import(&json!([{
      "name": "GitHub",
      "secret": "JBSWY3DPEHPK3PXP",
      "otp": { "account": "octocat", "tokenType": "TOTP", "period": 30, "counter": 0 }
    }]));
    assert!(matches!(account.otp(), OtpConfig::Totp { period: 30, .. }));
  }

  #[test]
  fn the_token_type_decides_and_is_case_insensitive() {
    let account = only_import(&json!([{
      "name": "Example", "secret": "JBSWY3DPEHPK3PXP",
      "otp": { "account": "bob", "tokenType": "hotp", "counter": 5 }
    }]));
    assert!(matches!(account.otp(), OtpConfig::Hotp { counter: 5, .. }));

    let account = only_import(&json!([{
      "name": "Example", "secret": "JBSWY3DPEHPK3PXP",
      "otp": { "account": "bob", "tokenType": "totp" }
    }]));
    assert!(matches!(account.otp(), OtpConfig::Totp { period: 30, .. }));
  }

  #[test]
  fn without_a_token_type_a_counter_means_hotp() {
    let account = only_import(&json!([{
      "name": "Example", "secret": "JBSWY3DPEHPK3PXP", "otp": { "account": "bob", "counter": 3 }
    }]));
    assert!(matches!(account.otp(), OtpConfig::Hotp { counter: 3, .. }));

    let account = only_import(&json!([{
      "name": "Example", "secret": "JBSWY3DPEHPK3PXP", "otp": { "account": "bob" }
    }]));
    assert!(matches!(account.otp(), OtpConfig::Totp { .. }));
  }

  #[test]
  fn unsupported_and_incomplete_token_types_are_skipped() {
    let outcome = import_plain(&json!([
      { "name": "Steam", "secret": "JBSWY3DPEHPK3PXP", "otp": { "tokenType": "STEAM" } },
      { "name": "HOTP", "secret": "JBSWY3DPEHPK3PXP", "otp": { "tokenType": "HOTP" } },
      {
        "name": "Exhausted", "secret": "JBSWY3DPEHPK3PXP",
        "otp": { "tokenType": "HOTP", "counter": u64::MAX }
      },
    ]));

    assert!(outcome.imported.is_empty());
    let reasons: Vec<&SkipReason> = outcome.skipped.iter().map(|s| &s.reason).collect();
    assert!(matches!(
      reasons[..],
      [
        SkipReason::Invalid(QrError::UnknownType(_)),
        SkipReason::Invalid(QrError::MissingCounter),
        SkipReason::Invalid(QrError::InvalidOtpConfig(OtpError::CounterExhausted)),
      ]
    ));
  }

  #[test]
  fn algorithm_and_secret_follow_the_shared_rules() {
    let account = only_import(&json!([{
      "name": "Example", "secret": "jbsw y3dp ehpk 3pxp",
      "otp": { "account": "bob", "algorithm": "sha512" }
    }]));
    assert!(matches!(
      account.otp(),
      OtpConfig::Totp {
        algorithm: Algorithm::Sha512,
        ..
      }
    ));
  }

  #[test]
  fn a_service_is_named_by_its_issuer_or_title() {
    // No `otp.issuer`: the service's title stands in.
    let account = only_import(&json!([{
      "name": "GitHub", "secret": "JBSWY3DPEHPK3PXP", "otp": { "account": "octocat" }
    }]));
    assert_eq!(account.issuer(), Some("GitHub"));
    assert_eq!(account.account_label(), "octocat");

    // No account name: the issuer alone names it.
    let account = only_import(&json!([{
      "name": "GitHub", "secret": "JBSWY3DPEHPK3PXP", "otp": { "issuer": "GitHub", "account": "" }
    }]));
    assert_eq!(account.issuer(), None);
    assert_eq!(account.account_label(), "GitHub");
  }

  #[test]
  fn a_malformed_service_is_skipped_and_the_rest_import() {
    let outcome = import_plain(&json!([
      { "name": "Good", "secret": "JBSWY3DPEHPK3PXP", "otp": { "account": "a" } },
      { "name": "Bad digits", "secret": "JBSWY3DPEHPK3PXP", "otp": { "digits": 300 } },
      { "name": "No secret" },
      42,
    ]));

    assert_eq!(outcome.imported.len(), 1);
    let skipped: Vec<(&str, &SkipReason)> = outcome
      .skipped
      .iter()
      .map(|s| (s.label.as_str(), &s.reason))
      .collect();
    assert!(matches!(
      skipped[..],
      [
        ("Bad digits", SkipReason::Malformed(_)),
        ("No secret", SkipReason::Invalid(QrError::MissingSecret)),
        ("entry 4", SkipReason::Malformed(_)),
      ]
    ));
  }

  #[test]
  fn falls_back_to_the_otp_link_when_the_secret_is_absent() {
    let account = only_import(&json!([{
      "name": "Example",
      "secret": "",
      "otp": {
        "link": "otpauth://totp/Example:alice@google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example"
      }
    }]));
    assert_eq!(account.account_label(), "alice@google.com");
  }

  #[test]
  fn duplicates_within_the_file_are_reported() {
    let service =
      json!({ "name": "GitHub", "secret": "JBSWY3DPEHPK3PXP", "otp": { "account": "octocat" } });
    let outcome = import_plain(&json!([service, service]));

    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.duplicates.len(), 1);
    assert_eq!(outcome.duplicates[0].label, "GitHub (octocat)");
    assert!(matches!(
      outcome.duplicates[0].reason,
      SkipReason::DuplicateInFile
    ));
  }

  #[test]
  fn a_file_that_is_not_a_backup_is_unrecognized() {
    for file in [
      &b"not json"[..],
      b"[]",
      br#"{"services": 3}"#,
      br#"{"version": 1, "header": {}, "db": {}}"#,
    ] {
      assert!(matches!(
        import_fresh(file, None),
        Err(InteropError::UnrecognizedFormat(_))
      ));
    }
  }
}
