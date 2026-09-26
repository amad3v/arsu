//! [`ParsedAccount`] and the rules every way of adding an account
//! shares.
//!
//! An account can come from an `otpauth://` URI (typed, pasted or read
//! from a QR code), from the manual-entry form, or from another app's
//! backup file (the `interop` crate). All of them decode secrets,
//! algorithm names and OTP types with the functions below and build the
//! account through [`ParsedAccount::new`], so the same input is accepted
//! or rejected the same way whichever path it arrives by.

use std::str::FromStr;

use zeroize::Zeroizing;

use otp::{Algorithm, Totp, hotp};
use vault_core::{OtpConfig, SecretBytes};

use crate::QrError;

/// Code length when the source doesn't specify one (the Key URI Format
/// default).
pub const DEFAULT_DIGITS: u8 = 6;

/// TOTP time step when the source doesn't specify one (the Key URI
/// Format default).
pub const DEFAULT_PERIOD_SECS: u64 = 30;

/// Hash algorithm when the source doesn't specify one (the Key URI
/// Format default).
pub const DEFAULT_ALGORITHM: Algorithm = Algorithm::Sha1;

/// Longest prefix of an unrecognized input value that an error message
/// repeats back. The value comes from untrusted input and may be
/// arbitrarily long.
const MAX_ECHOED_CHARS: usize = 32;

/// Which of the two OTP schemes an account uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OtpKind {
  /// Time-based (RFC 6238).
  Totp,
  /// Counter-based (RFC 4226).
  Hotp,
}

/// Parses `totp` or `hotp`, ignoring ASCII case: the Key URI Format
/// writes them in lowercase, Aegis too, and 2FAS in uppercase.
impl FromStr for OtpKind {
  type Err = QrError;

  fn from_str(name: &str) -> Result<Self, QrError> {
    if name.eq_ignore_ascii_case("totp") {
      Ok(Self::Totp)
    } else if name.eq_ignore_ascii_case("hotp") {
      Ok(Self::Hotp)
    } else {
      Err(QrError::UnknownType(excerpt(name)))
    }
  }
}

/// The parameter specific to an account's OTP scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtpParams {
  /// A new code every `period` seconds.
  Totp { period: u64 },
  /// `counter` is the next counter value to generate a code for.
  Hotp { counter: u64 },
}

impl OtpParams {
  /// Picks the parameter `kind` needs from whatever a source provides:
  /// TOTP falls back to [`DEFAULT_PERIOD_SECS`], HOTP requires a
  /// counter. The parameter of the other scheme is ignored, so a TOTP
  /// account that also carries a counter stays TOTP.
  ///
  /// # Errors
  ///
  /// Returns [`QrError::MissingCounter`] for HOTP without a counter.
  pub fn for_kind(
    kind: OtpKind,
    period: Option<u64>,
    counter: Option<u64>,
  ) -> Result<Self, QrError> {
    match kind {
      OtpKind::Totp => Ok(Self::Totp {
        period: period.unwrap_or(DEFAULT_PERIOD_SECS),
      }),
      OtpKind::Hotp => counter
        .map(|counter| Self::Hotp { counter })
        .ok_or(QrError::MissingCounter),
    }
  }
}

/// Parses an algorithm name as the Key URI Format writes it (`SHA1`,
/// `SHA256` or `SHA512`), ignoring ASCII case.
///
/// # Errors
///
/// Returns [`QrError::UnknownAlgorithm`] for any other name.
pub fn parse_algorithm(name: &str) -> Result<Algorithm, QrError> {
  [Algorithm::Sha1, Algorithm::Sha256, Algorithm::Sha512]
    .into_iter()
    .find(|&algorithm| algorithm_name(algorithm).eq_ignore_ascii_case(name))
    .ok_or_else(|| QrError::UnknownAlgorithm(excerpt(name)))
}

/// The Key URI Format name of `algorithm`, which is also what Aegis
/// and 2FAS write.
#[must_use]
pub const fn algorithm_name(algorithm: Algorithm) -> &'static str {
  match algorithm {
    Algorithm::Sha1 => "SHA1",
    Algorithm::Sha256 => "SHA256",
    Algorithm::Sha512 => "SHA512",
  }
}

/// Decodes a base32 (RFC 4648) OTP secret. Whitespace anywhere is
/// ignored, because services display secrets in groups. Letter case
/// does not matter, and trailing `=` padding is optional, because
/// generators disagree on both.
///
/// # Errors
///
/// Returns [`QrError::InvalidSecret`] if what remains is not base32.
pub fn decode_secret(encoded: &str) -> Result<SecretBytes, QrError> {
  // Case mapping keeps every character's length and whitespace is only
  // dropped, so the buffer never outgrows (and never leaves a copy
  // behind in) this allocation.
  let mut normalized = Zeroizing::new(String::with_capacity(encoded.len()));
  normalized.extend(
    encoded
      .chars()
      .filter(|c| !c.is_whitespace())
      .map(|c| c.to_ascii_uppercase()),
  );

  base32::decode(
    base32::Alphabet::Rfc4648 { padding: false },
    normalized.trim_end_matches('='),
  )
  .map(SecretBytes::from)
  .ok_or(QrError::InvalidSecret)
}

/// Encodes a secret as unpadded, uppercase base32, the form
/// `otpauth://` URIs and backup files carry it in.
#[must_use]
pub fn encode_secret(secret: &SecretBytes) -> Zeroizing<String> {
  Zeroizing::new(base32::encode(
    base32::Alphabet::Rfc4648 { padding: false },
    secret.expose_secret(),
  ))
}

/// The start of `value`, for repeating an unrecognized input back in
/// an error message.
fn excerpt(value: &str) -> String {
  let mut chars = value.chars();
  let mut excerpt: String = chars.by_ref().take(MAX_ECHOED_CHARS).collect();
  if chars.next().is_some() {
    excerpt.push('…');
  }
  excerpt
}

/// A validated account, ready to become a vault entry.
///
/// The only way to build one is [`ParsedAccount::new`], so every
/// instance has passed the label rules and the same checks
/// `otp::Totp::new` and `otp::hotp` apply (digit count, secret, period
/// and counter bounds): it can never become a vault entry the OTP
/// engine rejects the first time it generates a code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAccount {
  issuer: Option<String>,
  account_label: String,
  otp: OtpConfig,
}

impl ParsedAccount {
  /// Validates the parts of an account and assembles it.
  ///
  /// The label follows the Key URI Format rules, whatever the source:
  /// - surrounding whitespace is dropped, and a blank issuer is no
  ///   issuer;
  /// - the account needs a name; when only an issuer is given, it
  ///   becomes the account name;
  /// - neither part may contain `:`. An `otpauth://` label separates
  ///   the two parts with a colon and has no way to escape one, so such
  ///   an account could not be exported and scanned back unchanged.
  ///
  /// # Errors
  ///
  /// Returns [`QrError::MissingLabel`] if both the issuer and the
  /// account name are blank, [`QrError::LabelContainsColon`] if either
  /// contains `:`, and [`QrError::InvalidOtpConfig`] if the OTP engine
  /// rejects the parameters (for example an empty secret, 9 digits, or
  /// an HOTP counter that can no longer advance).
  pub fn new(
    issuer: Option<&str>,
    account_label: &str,
    secret: SecretBytes,
    algorithm: Algorithm,
    digits: u8,
    params: OtpParams,
  ) -> Result<Self, QrError> {
    let (issuer, account_label) = normalize_label(issuer, account_label)?;

    let otp = match params {
      OtpParams::Totp { period } => {
        Totp::new(secret.expose_secret(), algorithm, digits, period)?;
        OtpConfig::Totp {
          secret,
          algorithm,
          digits,
          period,
        }
      }
      OtpParams::Hotp { counter } => {
        hotp(secret.expose_secret(), counter, algorithm, digits)?;
        OtpConfig::Hotp {
          secret,
          algorithm,
          digits,
          counter,
        }
      }
    };

    Ok(Self {
      issuer,
      account_label,
      otp,
    })
  }

  /// The service the account belongs to, if known.
  #[must_use]
  pub fn issuer(&self) -> Option<&str> {
    self.issuer.as_deref()
  }

  /// The account name at that service (never empty).
  #[must_use]
  pub fn account_label(&self) -> &str {
    &self.account_label
  }

  #[must_use]
  pub fn otp(&self) -> &OtpConfig {
    &self.otp
  }

  /// Splits the account into issuer, account name and OTP
  /// configuration, e.g. to store it as a vault entry.
  #[must_use]
  pub fn into_parts(self) -> (Option<String>, String, OtpConfig) {
    (self.issuer, self.account_label, self.otp)
  }
}

/// The label rules documented on [`ParsedAccount::new`].
fn normalize_label(
  issuer: Option<&str>,
  account_label: &str,
) -> Result<(Option<String>, String), QrError> {
  let issuer = issuer.map(str::trim).filter(|issuer| !issuer.is_empty());
  let (issuer, account_label) = match account_label.trim() {
    "" => (None, issuer.ok_or(QrError::MissingLabel)?),
    account_label => (issuer, account_label),
  };

  if issuer
    .into_iter()
    .chain([account_label])
    .any(|part| part.contains(':'))
  {
    return Err(QrError::LabelContainsColon);
  }

  Ok((issuer.map(str::to_owned), account_label.to_owned()))
}

/// Input for adding an account by typing it in, the fallback for
/// services that show a text secret and no QR code. Every field is
/// explicit on purpose: defaults (6 digits, SHA1, 30 s) belong in the
/// form that collects this, not hidden inside the validation.
pub struct ManualEntryInput {
  pub issuer: Option<String>,
  pub account_label: String,
  /// The secret as the user typed or pasted it, in base32. Spaces and
  /// letter case don't matter (see [`decode_secret`]).
  pub secret_base32: String,
  pub algorithm: Algorithm,
  pub digits: u8,
  pub params: OtpParams,
}

/// Builds a [`ParsedAccount`] from manually entered fields, through
/// exactly the same decoding and validation as every other source.
///
/// # Errors
///
/// Returns an error if the secret is not base32 or the account fails
/// [`ParsedAccount::new`]'s validation.
pub fn parse_manual_entry(input: ManualEntryInput) -> Result<ParsedAccount, QrError> {
  let secret_text = Zeroizing::new(input.secret_base32);
  ParsedAccount::new(
    input.issuer.as_deref(),
    &input.account_label,
    decode_secret(&secret_text)?,
    input.algorithm,
    input.digits,
    input.params,
  )
}

#[cfg(test)]
mod tests {
  use otp::OtpError;

  use super::*;

  /// "Hello!\xDE\xAD\xBE\xEF", Google's Key URI Format example secret.
  const SECRET_BASE32: &str = "JBSWY3DPEHPK3PXP";

  fn secret() -> SecretBytes {
    decode_secret(SECRET_BASE32).unwrap()
  }

  fn totp_account(issuer: Option<&str>, account_label: &str) -> Result<ParsedAccount, QrError> {
    ParsedAccount::new(
      issuer,
      account_label,
      secret(),
      Algorithm::Sha1,
      6,
      OtpParams::Totp { period: 30 },
    )
  }

  fn manual_input(secret_base32: &str) -> ManualEntryInput {
    ManualEntryInput {
      issuer: Some("Example".to_string()),
      account_label: "alice@example.com".to_string(),
      secret_base32: secret_base32.to_string(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      params: OtpParams::Totp { period: 30 },
    }
  }

  // ---- secrets ---------------------------------------------------------

  #[test]
  fn decode_secret_ignores_case_whitespace_and_padding() {
    let reference = secret();
    for variant in [
      "jbswy3dpehpk3pxp",
      "JBSW Y3DP EHPK 3PXP",
      "jbsw\ty3dp\nehpk 3pxp",
      "JBSWY3DPEHPK3PXP======",
      "JBSW\u{a0}Y3DP\u{a0}EHPK\u{a0}3PXP",
    ] {
      assert_eq!(decode_secret(variant).unwrap(), reference, "{variant:?}");
    }
  }

  #[test]
  fn decode_secret_rejects_non_base32() {
    for invalid in [
      "not-valid-base32!!!",
      "JBSW=Y3DP",
      "JBSWY3DPEHPK3PX1",
      "ÄÖÜ",
    ] {
      assert!(
        matches!(decode_secret(invalid), Err(QrError::InvalidSecret)),
        "{invalid:?}"
      );
    }
  }

  #[test]
  fn encode_secret_inverts_decode_secret() {
    assert_eq!(*encode_secret(&secret()), SECRET_BASE32);
  }

  // ---- algorithm and type names ----------------------------------------

  #[test]
  fn algorithm_names_parse_in_any_case_and_round_trip() {
    for algorithm in [Algorithm::Sha1, Algorithm::Sha256, Algorithm::Sha512] {
      let name = algorithm_name(algorithm);
      assert_eq!(parse_algorithm(name).unwrap(), algorithm);
      assert_eq!(
        parse_algorithm(&name.to_ascii_lowercase()).unwrap(),
        algorithm
      );
    }
  }

  #[test]
  fn unknown_algorithm_is_rejected_with_a_bounded_excerpt() {
    assert!(matches!(
      parse_algorithm("MD5"),
      Err(QrError::UnknownAlgorithm(name)) if name == "MD5"
    ));

    let hostile = "X".repeat(10_000);
    let Err(QrError::UnknownAlgorithm(echoed)) = parse_algorithm(&hostile) else {
      panic!("expected UnknownAlgorithm");
    };
    assert_eq!(echoed.chars().count(), MAX_ECHOED_CHARS + 1);
    assert!(echoed.ends_with('…'));
  }

  #[test]
  fn otp_kind_parses_in_any_case() {
    assert_eq!("totp".parse::<OtpKind>().unwrap(), OtpKind::Totp);
    assert_eq!("TOTP".parse::<OtpKind>().unwrap(), OtpKind::Totp);
    assert_eq!("Hotp".parse::<OtpKind>().unwrap(), OtpKind::Hotp);
    assert!(matches!(
      "STEAM".parse::<OtpKind>(),
      Err(QrError::UnknownType(name)) if name == "STEAM"
    ));
  }

  #[test]
  fn otp_params_take_only_what_the_kind_needs() {
    assert_eq!(
      OtpParams::for_kind(OtpKind::Totp, None, Some(0)).unwrap(),
      OtpParams::Totp {
        period: DEFAULT_PERIOD_SECS
      }
    );
    assert_eq!(
      OtpParams::for_kind(OtpKind::Hotp, Some(30), Some(7)).unwrap(),
      OtpParams::Hotp { counter: 7 }
    );
    assert!(matches!(
      OtpParams::for_kind(OtpKind::Hotp, Some(30), None),
      Err(QrError::MissingCounter)
    ));
  }

  // ---- labels ----------------------------------------------------------

  #[test]
  fn label_parts_are_trimmed_and_a_blank_issuer_is_dropped() {
    let account = totp_account(Some("  Example "), " alice ").unwrap();
    assert_eq!(account.issuer(), Some("Example"));
    assert_eq!(account.account_label(), "alice");

    let account = totp_account(Some("   "), "alice").unwrap();
    assert_eq!(account.issuer(), None);
  }

  #[test]
  fn an_issuer_alone_becomes_the_account_name() {
    let account = totp_account(Some("GitHub"), "  ").unwrap();
    assert_eq!(account.issuer(), None);
    assert_eq!(account.account_label(), "GitHub");
  }

  #[test]
  fn an_empty_label_is_rejected() {
    for issuer in [None, Some(""), Some("  ")] {
      assert!(matches!(
        totp_account(issuer, " "),
        Err(QrError::MissingLabel)
      ));
    }
  }

  #[test]
  fn a_colon_in_either_label_part_is_rejected() {
    for (issuer, account_label) in [
      (Some("A:B"), "x"),
      (Some("A"), "user:1"),
      (None, "user:1"),
      (Some("Example:"), ""),
    ] {
      assert!(
        matches!(
          totp_account(issuer, account_label),
          Err(QrError::LabelContainsColon)
        ),
        "{issuer:?} / {account_label:?}"
      );
    }
  }

  // ---- OTP validation --------------------------------------------------

  #[test]
  fn otp_engine_validation_applies() {
    let invalid_digits = ParsedAccount::new(
      None,
      "alice",
      secret(),
      Algorithm::Sha1,
      20,
      OtpParams::Totp { period: 30 },
    );
    assert!(matches!(
      invalid_digits,
      Err(QrError::InvalidOtpConfig(OtpError::InvalidDigits(20)))
    ));

    let empty_secret = ParsedAccount::new(
      None,
      "alice",
      Vec::new().into(),
      Algorithm::Sha1,
      6,
      OtpParams::Totp { period: 30 },
    );
    assert!(matches!(
      empty_secret,
      Err(QrError::InvalidOtpConfig(OtpError::EmptySecret))
    ));
  }

  #[test]
  fn an_exhausted_hotp_counter_is_rejected() {
    let exhausted = ParsedAccount::new(
      None,
      "alice",
      secret(),
      Algorithm::Sha1,
      6,
      OtpParams::Hotp { counter: u64::MAX },
    );
    assert!(matches!(
      exhausted,
      Err(QrError::InvalidOtpConfig(OtpError::CounterExhausted))
    ));

    let last_usable = ParsedAccount::new(
      None,
      "alice",
      secret(),
      Algorithm::Sha1,
      6,
      OtpParams::Hotp {
        counter: u64::MAX - 1,
      },
    );
    assert!(last_usable.is_ok());
  }

  // ---- manual entry ----------------------------------------------------

  #[test]
  fn manual_entry_accepts_a_spaced_lowercase_secret() {
    let account = parse_manual_entry(manual_input("jbsw y3dp ehpk 3pxp")).unwrap();
    assert_eq!(account.issuer(), Some("Example"));
    assert_eq!(account.account_label(), "alice@example.com");
    assert_eq!(account.otp(), totp_account(None, "x").unwrap().otp());
  }

  #[test]
  fn manual_entry_rejects_an_invalid_secret() {
    assert!(matches!(
      parse_manual_entry(manual_input("not valid base32 at all!!!")),
      Err(QrError::InvalidSecret)
    ));
  }

  #[test]
  fn manual_entry_goes_through_the_shared_validation() {
    let mut input = manual_input(SECRET_BASE32);
    input.digits = 20;
    assert!(matches!(
      parse_manual_entry(input),
      Err(QrError::InvalidOtpConfig(_))
    ));

    let mut input = manual_input(SECRET_BASE32);
    input.account_label = "alice:work".to_string();
    assert!(matches!(
      parse_manual_entry(input),
      Err(QrError::LabelContainsColon)
    ));
  }

  #[test]
  fn manual_entry_supports_hotp() {
    let mut input = manual_input(SECRET_BASE32);
    input.params = OtpParams::Hotp { counter: 0 };
    let account = parse_manual_entry(input).unwrap();
    assert!(matches!(account.otp(), OtpConfig::Hotp { counter: 0, .. }));
  }
}
