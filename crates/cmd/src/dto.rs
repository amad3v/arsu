//! Every type that crosses the IPC boundary, in either direction.
//!
//! These are deliberately separate from the internal types they are
//! built from (`vault_core::Entry`, `otp::Algorithm`, …): this module is
//! the one place that answers "exactly what does the frontend see", and
//! a change to an internal representation cannot silently change the
//! wire format. The one exception is [`Theme`], whose lowercase form is
//! the same in the settings file and on the wire by design. Each type is
//! named after its TypeScript mirror in `src/types/api.ts`, and
//! the tests at the bottom pin the JSON shapes.
//!
//! Secret material crosses IPC in exactly these places, and nowhere else:
//! - inbound, as [`SecretString`]: master and export passwords, the
//!   secret of a manually entered account, and `otpauth://` URIs (which
//!   carry the secret). The user types or pastes these into the `WebView`,
//!   so they cannot avoid it.
//! - outbound, as [`SecretQrSvg`]: the QR code of a single entry, which
//!   encodes its secret. It is only produced after the master password
//!   has been re-verified (`AppState::export_entry_qr`).
//!
//! Everything else going out is non-secret: summaries, codes, ids.

use std::{fmt, mem};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use zeroize::Zeroizing;

use otp::Totp;
pub use storage::Theme;
use vault_core::{Entry, OtpConfig};

use crate::error::AppError;

/// Text that is, or contains, a secret: a password, a base32 secret or
/// an `otpauth://` URI. Zeroized when dropped, and redacted in `Debug`.
///
/// Deserialized from a JSON string. The JSON the string was parsed from
/// is Tauri's to keep or free, so this bounds how long *this* copy
/// lives rather than guaranteeing no other copy exists.
pub struct SecretString(Zeroizing<String>);

impl SecretString {
  #[must_use]
  pub fn expose(&self) -> &str {
    &self.0
  }

  /// Hands the text over without copying it, for an API that takes a
  /// `String` and zeroizes it itself (as `qr::parse_manual_entry` does).
  fn into_string(mut self) -> String {
    mem::take(&mut *self.0)
  }
}

impl From<String> for SecretString {
  fn from(text: String) -> Self {
    Self(Zeroizing::new(text))
  }
}

impl fmt::Debug for SecretString {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("SecretString(<redacted>)")
  }
}

impl<'de> Deserialize<'de> for SecretString {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    String::deserialize(deserializer).map(Self::from)
  }
}

/// A single entry's QR code, as an SVG document: the one response that
/// carries secret material, because the QR code encodes the entry's
/// `otpauth://` URI, secret included. Serialized as a plain string;
/// zeroized when dropped.
pub struct SecretQrSvg(Zeroizing<String>);

impl From<String> for SecretQrSvg {
  fn from(svg: String) -> Self {
    Self(Zeroizing::new(svg))
  }
}

impl SecretQrSvg {
  #[must_use]
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl fmt::Debug for SecretQrSvg {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("SecretQrSvg(<redacted>)")
  }
}

impl Serialize for SecretQrSvg {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&self.0)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OtpType {
  Totp,
  Hotp,
}

/// One vault entry as the entry list shows it: everything but the
/// secret and the HOTP counter.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
  pub id: String,
  pub issuer: Option<String>,
  pub account_label: String,
  pub otp_type: OtpType,
  pub digits: u8,
  /// The TOTP time step in seconds; `None` for HOTP.
  pub period: Option<u64>,
}

impl From<&Entry> for EntrySummary {
  fn from(entry: &Entry) -> Self {
    let (otp_type, digits, period) = match &entry.otp {
      OtpConfig::Totp { digits, period, .. } => (OtpType::Totp, *digits, Some(*period)),
      OtpConfig::Hotp { digits, .. } => (OtpType::Hotp, *digits, None),
    };
    Self {
      id: entry.id.to_string(),
      issuer: entry.issuer.clone(),
      account_label: entry.account_label.clone(),
      otp_type,
      digits,
      period,
    }
  }
}

/// A computed code: the only form an entry's secret takes on its way
/// to the frontend, besides [`SecretQrSvg`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeResponse {
  pub code: String,
  /// Seconds until `code` expires (`1..=period`); `None` for HOTP, whose
  /// codes do not expire on a timer.
  pub expires_in_seconds: Option<u64>,
  /// The code that replaces `code` when it expires; `None` for HOTP.
  pub next_code: Option<String>,
}

impl CodeResponse {
  /// The TOTP code for the time step containing `unix_time`, with its
  /// successor.
  #[must_use]
  pub fn totp(totp: &Totp<'_>, unix_time: u64) -> Self {
    Self {
      code: totp.generate(unix_time),
      expires_in_seconds: Some(totp.seconds_remaining(unix_time)),
      next_code: Some(totp.next_code(unix_time)),
    }
  }

  #[must_use]
  pub fn hotp(code: String) -> Self {
    Self {
      code,
      expires_in_seconds: None,
      next_code: None,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
  Sha1,
  Sha256,
  Sha512,
}

impl From<Algorithm> for otp::Algorithm {
  fn from(algorithm: Algorithm) -> Self {
    match algorithm {
      Algorithm::Sha1 => Self::Sha1,
      Algorithm::Sha256 => Self::Sha256,
      Algorithm::Sha512 => Self::Sha512,
    }
  }
}

/// The scheme-specific half of [`ManualEntryInput`], flattened into it:
/// `"type": "totp"` with a `period`, or `"type": "hotp"` with a
/// `counter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ManualOtpParams {
  Totp { period: u64 },
  Hotp { counter: u64 },
}

impl From<ManualOtpParams> for qr::OtpParams {
  fn from(params: ManualOtpParams) -> Self {
    match params {
      ManualOtpParams::Totp { period } => Self::Totp { period },
      ManualOtpParams::Hotp { counter } => Self::Hotp { counter },
    }
  }
}

/// What the "add account manually" form sends. There are no defaults
/// here on purpose: the form pre-fills the usual values (SHA1, 6 digits,
/// 30 s) and sends what the user confirmed.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualEntryInput {
  pub issuer: Option<String>,
  pub account_label: String,
  pub secret_base32: SecretString,
  pub algorithm: Algorithm,
  pub digits: u8,
  #[serde(flatten)]
  pub params: ManualOtpParams,
}

impl From<ManualEntryInput> for qr::ManualEntryInput {
  fn from(input: ManualEntryInput) -> Self {
    Self {
      issuer: input.issuer,
      account_label: input.account_label,
      secret_base32: input.secret_base32.into_string(),
      algorithm: input.algorithm.into(),
      digits: input.digits,
      params: input.params.into(),
    }
  }
}

/// Parses an entry id sent by the frontend.
///
/// # Errors
///
/// Returns [`AppError::InvalidEntryId`] if `id` is not a UUID.
pub fn parse_entry_id(id: &str) -> Result<Uuid, AppError> {
  Uuid::parse_str(id).map_err(|_| AppError::InvalidEntryId(id.to_owned()))
}

/// Another authenticator's backup format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
  /// An Aegis vault export (`.json`), plain or encrypted.
  Aegis,
  /// A 2FAS backup (`.2fas`), plain or encrypted.
  Twofas,
}

/// A file the user picked for import. The path stays in Rust; the
/// frontend gets a token to import it with, and a name to show.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
  pub token: String,
  /// The file name, for display only (lossily converted to UTF-8).
  pub file_name: String,
  /// The backup's format, from its extension.
  pub format: ImportFormat,
}

/// The result of an import: the ids of the new entries, and the entries
/// left out, each with the reason.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
  pub imported_ids: Vec<String>,
  /// Entries that are not valid accounts.
  pub skipped: Vec<SkippedEntry>,
  /// Valid accounts already in the vault, or repeated in the file.
  pub duplicates: Vec<SkippedEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
  /// How the source file names the entry.
  pub label: String,
  /// Why it was left out, written for the user.
  pub reason: String,
}

impl From<&interop::SkippedEntry> for SkippedEntry {
  fn from(skipped: &interop::SkippedEntry) -> Self {
    Self {
      label: skipped.label.clone(),
      reason: skipped.reason.to_string(),
    }
  }
}

/// What the About dialog shows: the app, what it runs on, and where it
/// keeps its files. Paths are lossily converted to UTF-8, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
  pub name: String,
  pub version: String,
  pub tauri_version: String,
  /// The `WebKitGTK` version, or `None` if it cannot be read.
  pub webview_version: Option<String>,
  pub vault_path: String,
  pub settings_path: String,
}

/// A page of the project's the About dialog links to. The frontend names
/// one; the URL itself is fixed in Rust, so the `WebView` can't have any
/// other address opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppLink {
  Website,
  Issues,
}

/// The settings in effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  pub theme: Theme,
  pub auto_lock_minutes: u32,
  pub clipboard_clear_seconds: u32,
}

impl From<&storage::Settings> for Settings {
  fn from(settings: &storage::Settings) -> Self {
    Self {
      theme: settings.theme,
      auto_lock_minutes: settings.auto_lock_minutes.get(),
      clipboard_clear_seconds: settings.clipboard_clear_seconds.get(),
    }
  }
}

/// A change to the settings: each field that is present (and not
/// `null`) replaces the current value; the others are kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
  #[serde(default)]
  pub theme: Option<Theme>,
  #[serde(default)]
  pub auto_lock_minutes: Option<u32>,
  #[serde(default)]
  pub clipboard_clear_seconds: Option<u32>,
}

impl SettingsUpdate {
  /// `settings` with this update applied.
  ///
  /// # Errors
  ///
  /// Returns [`storage::InvalidSetting`] if a value is outside what the
  /// app supports.
  pub fn apply_to(
    self,
    settings: &storage::Settings,
  ) -> Result<storage::Settings, storage::InvalidSetting> {
    Ok(storage::Settings {
      theme: self.theme.unwrap_or(settings.theme),
      auto_lock_minutes: self
        .auto_lock_minutes
        .map_or(Ok(settings.auto_lock_minutes), TryInto::try_into)?,
      clipboard_clear_seconds: self
        .clipboard_clear_seconds
        .map_or(Ok(settings.clipboard_clear_seconds), TryInto::try_into)?,
    })
  }
}

#[cfg(test)]
mod tests {
  use serde_json::{Value, json};
  use storage::{AutoLockMinutes, ClipboardClearSeconds, InvalidSetting};

  use super::*;

  fn wire<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
  }

  fn entry(otp: OtpConfig) -> Entry {
    Entry {
      id: Uuid::nil(),
      issuer: Some("Example".to_owned()),
      account_label: "alice".to_owned(),
      otp,
      icon: None,
      tags: Vec::new(),
      notes: None,
      created_at: 0,
      updated_at: 0,
      deleted_at: None,
    }
  }

  #[test]
  fn entry_summaries_carry_the_period_but_never_the_secret_or_counter() {
    let totp = entry(OtpConfig::Totp {
      secret: b"secret".to_vec().into(),
      algorithm: otp::Algorithm::Sha1,
      digits: 6,
      period: 45,
    });
    let hotp = entry(OtpConfig::Hotp {
      secret: b"secret".to_vec().into(),
      algorithm: otp::Algorithm::Sha1,
      digits: 8,
      counter: 7,
    });

    assert_eq!(
      wire(&EntrySummary::from(&totp)),
      json!({
        "id": "00000000-0000-0000-0000-000000000000",
        "issuer": "Example",
        "accountLabel": "alice",
        "otpType": "totp",
        "digits": 6,
        "period": 45,
      })
    );
    assert_eq!(
      wire(&EntrySummary::from(&hotp)),
      json!({
        "id": "00000000-0000-0000-0000-000000000000",
        "issuer": "Example",
        "accountLabel": "alice",
        "otpType": "hotp",
        "digits": 8,
        "period": null,
      })
    );
  }

  #[test]
  fn totp_codes_come_with_their_expiry_and_successor() {
    // RFC 6238 Appendix B, SHA1, truncated to 6 digits: T = 59 is in
    // the step [30, 60), the next step starts at 60.
    let totp = Totp::new(b"12345678901234567890", otp::Algorithm::Sha1, 6, 30).unwrap();

    assert_eq!(
      wire(&CodeResponse::totp(&totp, 59)),
      json!({ "code": "287082", "expiresInSeconds": 1, "nextCode": totp.generate(60) })
    );
    assert_eq!(
      wire(&CodeResponse::hotp("755224".to_owned())),
      json!({ "code": "755224", "expiresInSeconds": null, "nextCode": null })
    );
  }

  #[test]
  fn manual_entries_are_flat_with_a_type_tag() {
    let totp: ManualEntryInput = serde_json::from_value(json!({
      "issuer": null,
      "accountLabel": "alice",
      "secretBase32": "JBSWY3DPEHPK3PXP",
      "algorithm": "sha256",
      "digits": 6,
      "type": "totp",
      "period": 30,
    }))
    .unwrap();
    assert_eq!(totp.algorithm, Algorithm::Sha256);
    assert_eq!(totp.params, ManualOtpParams::Totp { period: 30 });
    assert_eq!(totp.secret_base32.expose(), "JBSWY3DPEHPK3PXP");

    let hotp: ManualEntryInput = serde_json::from_value(json!({
      "issuer": "Example",
      "accountLabel": "alice",
      "secretBase32": "JBSWY3DPEHPK3PXP",
      "algorithm": "sha1",
      "digits": 6,
      "type": "hotp",
      "counter": 5,
    }))
    .unwrap();
    assert_eq!(hotp.params, ManualOtpParams::Hotp { counter: 5 });

    let converted = qr::ManualEntryInput::from(hotp);
    assert_eq!(converted.secret_base32, "JBSWY3DPEHPK3PXP");
    assert_eq!(converted.params, qr::OtpParams::Hotp { counter: 5 });
  }

  #[test]
  fn secrets_are_redacted_in_debug_output() {
    let input: ManualEntryInput = serde_json::from_value(json!({
      "issuer": null,
      "accountLabel": "alice",
      "secretBase32": "JBSWY3DPEHPK3PXP",
      "algorithm": "sha1",
      "digits": 6,
      "type": "totp",
      "period": 30,
    }))
    .unwrap();
    let debug = format!("{input:?}");
    assert!(!debug.contains("JBSWY3DPEHPK3PXP"), "{debug}");
    assert!(debug.contains("<redacted>"), "{debug}");

    let svg = SecretQrSvg::from("<svg>secret</svg>".to_owned());
    assert_eq!(format!("{svg:?}"), "SecretQrSvg(<redacted>)");
  }

  #[test]
  fn a_qr_svg_goes_out_as_a_plain_string() {
    let svg = SecretQrSvg::from("<svg/>".to_owned());
    assert_eq!(wire(&svg), json!("<svg/>"));
  }

  #[test]
  fn import_types_have_the_documented_shapes() {
    assert_eq!(wire(&ImportFormat::Twofas), json!("twofas"));

    let picked = PickedFile {
      token: "t".to_owned(),
      file_name: "backup.json".to_owned(),
      format: ImportFormat::Aegis,
    };
    assert_eq!(
      wire(&picked),
      json!({ "token": "t", "fileName": "backup.json", "format": "aegis" })
    );

    let summary = ImportSummary {
      imported_ids: vec!["id".to_owned()],
      skipped: vec![SkippedEntry {
        label: "a".to_owned(),
        reason: "why".to_owned(),
      }],
      duplicates: vec![SkippedEntry::from(&interop::SkippedEntry {
        label: "b".to_owned(),
        reason: interop::SkipReason::AlreadyInVault,
      })],
    };
    assert_eq!(
      wire(&summary),
      json!({
        "importedIds": ["id"],
        "skipped": [{ "label": "a", "reason": "why" }],
        "duplicates": [{ "label": "b", "reason": "already in the vault" }],
      })
    );
  }

  #[test]
  fn settings_go_out_in_camel_case() {
    assert_eq!(
      wire(&Settings::from(&storage::Settings::default())),
      json!({ "theme": "system", "autoLockMinutes": 5, "clipboardClearSeconds": 20 })
    );
  }

  #[test]
  fn a_settings_update_changes_only_the_fields_it_names() {
    let update: SettingsUpdate = serde_json::from_value(json!({ "autoLockMinutes": 15 })).unwrap();
    let current = storage::Settings {
      theme: Theme::Dark,
      ..storage::Settings::default()
    };

    let updated = update.apply_to(&current).unwrap();

    assert_eq!(updated.theme, Theme::Dark);
    assert_eq!(updated.auto_lock_minutes.get(), 15);
    assert_eq!(
      updated.clipboard_clear_seconds,
      ClipboardClearSeconds::DEFAULT
    );

    let everything: SettingsUpdate = serde_json::from_value(json!({
      "theme": "light", "autoLockMinutes": 1, "clipboardClearSeconds": 60,
    }))
    .unwrap();
    assert_eq!(
      everything.apply_to(&current).unwrap(),
      storage::Settings {
        theme: Theme::Light,
        auto_lock_minutes: AutoLockMinutes::try_from(1).unwrap(),
        clipboard_clear_seconds: ClipboardClearSeconds::try_from(60).unwrap(),
      }
    );
  }

  #[test]
  fn a_settings_update_rejects_unsupported_values() {
    let update = SettingsUpdate {
      auto_lock_minutes: Some(3),
      ..SettingsUpdate::default()
    };
    assert_eq!(
      update.apply_to(&storage::Settings::default()),
      Err(InvalidSetting::AutoLockMinutes(3))
    );

    let update = SettingsUpdate {
      clipboard_clear_seconds: Some(5),
      ..SettingsUpdate::default()
    };
    assert_eq!(
      update.apply_to(&storage::Settings::default()),
      Err(InvalidSetting::ClipboardClearSeconds(5))
    );
    assert!(serde_json::from_value::<SettingsUpdate>(json!({ "theme": "drak" })).is_err());
  }

  #[test]
  fn entry_ids_must_be_uuids() {
    assert_eq!(
      parse_entry_id("00000000-0000-0000-0000-000000000000").unwrap(),
      Uuid::nil()
    );
    assert!(matches!(
      parse_entry_id("nope"),
      Err(AppError::InvalidEntryId(id)) if id == "nope"
    ));
  }

  #[test]
  fn about_info_and_links_on_the_wire() {
    let info = AboutInfo {
      name: "Arsu".to_owned(),
      version: "1.0.0".to_owned(),
      tauri_version: "2.11.6".to_owned(),
      webview_version: Some("2.50.1".to_owned()),
      vault_path: "/v".to_owned(),
      settings_path: "/s".to_owned(),
    };
    assert_eq!(
      wire(&info),
      json!({
        "name": "Arsu",
        "version": "1.0.0",
        "tauriVersion": "2.11.6",
        "webviewVersion": "2.50.1",
        "vaultPath": "/v",
        "settingsPath": "/s",
      })
    );

    let link: AppLink = serde_json::from_value(json!("issues")).unwrap();
    assert_eq!(link, AppLink::Issues);
    // Only the named pages: never an address.
    assert!(serde_json::from_value::<AppLink>(json!("https://example.com")).is_err());
  }
}
