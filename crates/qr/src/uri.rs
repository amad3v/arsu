//! The `otpauth://` Key URI Format, originally Google Authenticator's
//! and now what every authenticator app reads and writes.

use std::{borrow::Cow, collections::HashMap, str::FromStr};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use zeroize::Zeroizing;

use vault_core::OtpConfig;

use crate::{
  QrError,
  account::{
    DEFAULT_ALGORITHM, DEFAULT_DIGITS, OtpKind, OtpParams, ParsedAccount, algorithm_name,
    decode_secret, encode_secret, parse_algorithm,
  },
};

const SCHEME: &str = "otpauth://";

/// Percent-encodes everything except RFC 3986's unreserved characters,
/// which RFC 3986 §2.3 says producers should not encode. `:` is
/// encoded, so the one literal colon in a label is the separator.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'-')
  .remove(b'.')
  .remove(b'_')
  .remove(b'~');

/// Decoded query parameters. Values borrow from the URI unless they
/// had to be percent-decoded, so the secret is not copied needlessly.
type Query<'uri> = HashMap<Cow<'uri, str>, Cow<'uri, str>>;

/// Parses an `otpauth://totp/...` or `otpauth://hotp/...` URI.
///
/// Hand-rolled rather than built on the `url` crate: `otpauth` is not
/// a hierarchical URL (no host, no internationalized domains), and a
/// full RFC 3986 parser pulls in the `idna`/ICU stack, which is a lot
/// of code and attack surface for `scheme://type/label?query`.
///
/// Every parameter present must be well-formed, even one the OTP type
/// does not use.
///
/// # Errors
///
/// Returns an error if the URI does not use the `otpauth://` scheme,
/// has no label or an undecodable one, has no secret or one that is
/// not base32, names an unknown OTP type or algorithm, has a numeric
/// parameter that does not parse, or fails [`ParsedAccount::new`]'s
/// validation (label rules, OTP parameters).
pub fn parse_otpauth_uri(uri: &str) -> Result<ParsedAccount, QrError> {
  let rest = uri.strip_prefix(SCHEME).ok_or(QrError::NotOtpauth)?;
  let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
  let (otp_type, label) = path.split_once('/').ok_or(QrError::MissingLabel)?;
  let kind: OtpKind = otp_type.parse()?;

  let label = percent_decode_str(label)
    .decode_utf8()
    .map_err(|_| QrError::InvalidLabelEncoding)?;
  // "label = accountname / issuer (':' / '%3A') *'%20' accountname".
  // Decoding first accepts both separator spellings; neither part may
  // contain a colon itself, which `ParsedAccount::new` enforces.
  let (label_issuer, account_label) = match label.split_once(':') {
    Some((issuer, account_label)) => (Some(issuer), account_label),
    None => (None, &*label),
  };

  let params = parse_query(query);
  // The `issuer` parameter wins over the label prefix: generators
  // disagree about which of the two they set.
  let issuer = params.get("issuer").map(AsRef::as_ref).or(label_issuer);
  let secret = decode_secret(params.get("secret").ok_or(QrError::MissingSecret)?)?;
  let algorithm = params
    .get("algorithm")
    .map_or(Ok(DEFAULT_ALGORITHM), |name| parse_algorithm(name))?;
  let digits = parse_number(&params, "digits")?.unwrap_or(DEFAULT_DIGITS);
  let otp_params = OtpParams::for_kind(
    kind,
    parse_number(&params, "period")?,
    parse_number(&params, "counter")?,
  )?;

  ParsedAccount::new(issuer, account_label, secret, algorithm, digits, otp_params)
}

/// Minimal `key=value&key=value` query-string parser, not a general
/// URL query parser (no repeated keys, no `+` as space): every
/// `otpauth://` parameter is single-valued and percent-encoded, not
/// form-encoded. A segment that isn't a well-formed pair is skipped.
fn parse_query(query: &str) -> Query<'_> {
  query
    .split('&')
    .filter_map(|pair| {
      let (key, value) = pair.split_once('=')?;
      let key = percent_decode_str(key).decode_utf8().ok()?;
      let value = percent_decode_str(value).decode_utf8().ok()?;
      Some((key, value))
    })
    .collect()
}

fn parse_number<T: FromStr>(params: &Query<'_>, name: &'static str) -> Result<Option<T>, QrError> {
  params
    .get(name)
    .map(|value| value.parse().map_err(|_| QrError::InvalidNumber(name)))
    .transpose()
}

/// Builds the `otpauth://` URI for one account, the counterpart to
/// [`parse_otpauth_uri`]. Used to export a single entry; bulk export
/// goes through an encrypted backup file instead.
///
/// The URI carries the secret, so it comes back in a buffer that is
/// zeroized on drop. Issuer and account name are encoded separately
/// and joined with a literal `:`. Accounts built by
/// [`ParsedAccount::new`] never contain a colon in either part, so
/// their URI parses back to the same account.
#[must_use]
pub fn build_otpauth_uri(
  issuer: Option<&str>,
  account_label: &str,
  otp: &OtpConfig,
) -> Zeroizing<String> {
  let (otp_type, secret, algorithm, digits, (param_key, param_value)) = match otp {
    OtpConfig::Totp {
      secret,
      algorithm,
      digits,
      period,
    } => (
      "totp",
      secret,
      *algorithm,
      *digits,
      ("&period=", period.to_string()),
    ),
    OtpConfig::Hotp {
      secret,
      algorithm,
      digits,
      counter,
    } => (
      "hotp",
      secret,
      *algorithm,
      *digits,
      ("&counter=", counter.to_string()),
    ),
  };

  let account_label = utf8_percent_encode(account_label, COMPONENT).to_string();
  let (label, issuer_param) = match issuer {
    Some(issuer) => {
      let issuer = utf8_percent_encode(issuer, COMPONENT).to_string();
      (
        format!("{issuer}:{account_label}"),
        format!("&issuer={issuer}"),
      )
    }
    None => (account_label, String::new()),
  };
  let secret = encode_secret(secret);
  let digits = digits.to_string();

  // `concat` sizes the buffer once, so no partial copy of the secret
  // is left behind by a reallocation.
  Zeroizing::new(
    [
      SCHEME,
      otp_type,
      "/",
      &label,
      "?secret=",
      &secret,
      &issuer_param,
      "&algorithm=",
      algorithm_name(algorithm),
      "&digits=",
      &digits,
      param_key,
      &param_value,
    ]
    .concat(),
  )
}

#[cfg(test)]
mod tests {
  use otp::{Algorithm, OtpError};
  use vault_core::SecretBytes;

  use super::*;

  // Both URIs are Google's own examples from the Key URI Format
  // specification.
  const SIMPLE_URI: &str =
    "otpauth://totp/Example:alice@google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example";

  const FULL_URI: &str = "otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA1&digits=6&period=30";

  const SEED: &[u8] = b"12345678901234567890";

  fn totp(seed: &[u8]) -> OtpConfig {
    OtpConfig::Totp {
      secret: SecretBytes::from(seed.to_vec()),
      algorithm: Algorithm::Sha256,
      digits: 8,
      period: 60,
    }
  }

  #[test]
  fn parses_the_simple_reference_example() {
    let account = parse_otpauth_uri(SIMPLE_URI).unwrap();
    assert_eq!(account.issuer(), Some("Example"));
    assert_eq!(account.account_label(), "alice@google.com");
    let OtpConfig::Totp {
      algorithm,
      digits,
      period,
      ..
    } = account.otp()
    else {
      panic!("expected Totp");
    };
    // The specification's defaults.
    assert_eq!(*algorithm, Algorithm::Sha1);
    assert_eq!(*digits, 6);
    assert_eq!(*period, 30);
  }

  #[test]
  fn parses_the_fully_specified_reference_example() {
    let account = parse_otpauth_uri(FULL_URI).unwrap();
    assert_eq!(account.issuer(), Some("ACME Co"));
    assert_eq!(account.account_label(), "john.doe@email.com");
    let OtpConfig::Totp { secret, .. } = account.otp() else {
      panic!("expected Totp");
    };
    let expected = base32::decode(
      base32::Alphabet::Rfc4648 { padding: false },
      "HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ",
    )
    .unwrap();
    assert_eq!(secret.expose_secret(), expected);
  }

  #[test]
  fn parses_hotp_with_counter() {
    let uri = "otpauth://hotp/Example:bob@example.com?secret=JBSWY3DPEHPK3PXP&counter=5";
    let account = parse_otpauth_uri(uri).unwrap();
    assert!(matches!(account.otp(), OtpConfig::Hotp { counter: 5, .. }));
  }

  #[test]
  fn accepts_the_encoded_separator_and_case_variants() {
    let uri = "otpauth://TOTP/Example%3A%20alice@google.com?secret=jbsw%20y3dp%20ehpk%203pxp&algorithm=sha256";
    let account = parse_otpauth_uri(uri).unwrap();
    assert_eq!(account.issuer(), Some("Example"));
    assert_eq!(account.account_label(), "alice@google.com");
    assert!(matches!(
      account.otp(),
      OtpConfig::Totp {
        algorithm: Algorithm::Sha256,
        ..
      }
    ));
  }

  #[test]
  fn rejects_non_otpauth_scheme() {
    assert!(matches!(
      parse_otpauth_uri("https://example.com"),
      Err(QrError::NotOtpauth)
    ));
  }

  #[test]
  fn rejects_missing_secret() {
    let uri = "otpauth://totp/Example:alice@example.com?issuer=Example";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::MissingSecret)
    ));
  }

  #[test]
  fn rejects_invalid_base32_secret() {
    let uri = "otpauth://totp/Example:alice@example.com?secret=not-valid-base32!!!";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::InvalidSecret)
    ));
  }

  #[test]
  fn rejects_hotp_missing_counter() {
    let uri = "otpauth://hotp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::MissingCounter)
    ));
  }

  #[test]
  fn rejects_an_exhausted_hotp_counter() {
    let uri = "otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP&counter=18446744073709551615";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::InvalidOtpConfig(OtpError::CounterExhausted))
    ));
  }

  #[test]
  fn rejects_unknown_type() {
    let uri = "otpauth://motp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::UnknownType(_))
    ));
  }

  #[test]
  fn rejects_unparseable_numbers() {
    let uri = "otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP&digits=300";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::InvalidNumber("digits"))
    ));
  }

  #[test]
  fn rejects_absurd_digit_count_via_otp_engine_validation() {
    let uri = "otpauth://totp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP&digits=20";
    assert!(matches!(
      parse_otpauth_uri(uri),
      Err(QrError::InvalidOtpConfig(_))
    ));
  }

  #[test]
  fn skips_a_malformed_query_segment() {
    let uri = "otpauth://totp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP&garbage";
    assert!(parse_otpauth_uri(uri).is_ok());
  }

  #[test]
  fn rejects_an_empty_label() {
    for uri in [
      "otpauth://totp/?secret=JBSWY3DPEHPK3PXP",
      "otpauth://totp/%20?secret=JBSWY3DPEHPK3PXP",
      "otpauth://totp/:?secret=JBSWY3DPEHPK3PXP",
      "otpauth://totp?secret=JBSWY3DPEHPK3PXP",
    ] {
      assert!(
        matches!(parse_otpauth_uri(uri), Err(QrError::MissingLabel)),
        "{uri}"
      );
    }
  }

  #[test]
  fn an_issuer_only_label_names_the_account() {
    let account = parse_otpauth_uri("otpauth://totp/GitHub:?secret=JBSWY3DPEHPK3PXP").unwrap();
    assert_eq!(account.issuer(), None);
    assert_eq!(account.account_label(), "GitHub");
  }

  #[test]
  fn rejects_a_label_part_containing_a_colon() {
    for uri in [
      // Issuer "A:B" encoded, then the separator: the account is "B:x".
      "otpauth://totp/A%3AB:x?secret=JBSWY3DPEHPK3PXP",
      "otpauth://totp/Example:user:1?secret=JBSWY3DPEHPK3PXP",
      "otpauth://totp/alice?secret=JBSWY3DPEHPK3PXP&issuer=A%3AB",
    ] {
      assert!(
        matches!(parse_otpauth_uri(uri), Err(QrError::LabelContainsColon)),
        "{uri}"
      );
    }
  }

  #[test]
  fn build_joins_separately_encoded_label_parts_with_a_literal_colon() {
    let uri = build_otpauth_uri(Some("ACME Co"), "john.doe@email.com", &totp(SEED));
    assert!(
      uri.starts_with("otpauth://totp/ACME%20Co:john.doe%40email.com?secret="),
      "{}",
      *uri
    );
    assert!(uri.contains("&issuer=ACME%20Co&"));
  }

  #[test]
  fn build_then_parse_round_trips_totp() {
    let original = ParsedAccount::new(
      Some("My Service"),
      "user@example.com",
      SEED.to_vec().into(),
      Algorithm::Sha256,
      8,
      OtpParams::Totp { period: 60 },
    )
    .unwrap();
    let uri = build_otpauth_uri(original.issuer(), original.account_label(), original.otp());
    assert_eq!(parse_otpauth_uri(&uri).unwrap(), original);
  }

  #[test]
  fn build_then_parse_round_trips_hotp_without_issuer() {
    let original = ParsedAccount::new(
      None,
      "no-issuer@example.com",
      SEED.to_vec().into(),
      Algorithm::Sha1,
      6,
      OtpParams::Hotp { counter: 17 },
    )
    .unwrap();
    let uri = build_otpauth_uri(None, original.account_label(), original.otp());
    assert_eq!(parse_otpauth_uri(&uri).unwrap(), original);
  }

  /// Property: every account `ParsedAccount::new` accepts survives
  /// `build_otpauth_uri` → `parse_otpauth_uri` unchanged. Labels are
  /// generated from fragments chosen to collide with URI syntax
  /// (separators, escapes, spaces, non-ASCII), in every combination of
  /// one issuer fragment and up to two account fragments.
  #[test]
  fn build_then_parse_round_trips_every_valid_label() {
    const FRAGMENTS: [&str; 20] = [
      "",
      " ",
      "a",
      "Z9",
      "@",
      "&",
      "%",
      "%3A",
      "%2",
      "?",
      "#",
      "/",
      "+",
      "=",
      ".-_~",
      "é",
      "日本",
      "\u{1F600}",
      "\u{a0}",
      ";,",
    ];
    let otp = totp(SEED);
    let mut checked = 0_u32;

    for issuer in std::iter::once(None).chain(FRAGMENTS.map(Some)) {
      for first in FRAGMENTS {
        for second in FRAGMENTS {
          let account_label = format!("{first}{second}");
          let Ok(original) = ParsedAccount::new(
            issuer,
            &account_label,
            SEED.to_vec().into(),
            Algorithm::Sha256,
            8,
            OtpParams::Totp { period: 60 },
          ) else {
            continue;
          };
          assert_eq!(original.otp(), &otp);

          let uri = build_otpauth_uri(original.issuer(), original.account_label(), original.otp());
          let parsed = parse_otpauth_uri(&uri).unwrap_or_else(|error| {
            panic!("{issuer:?} / {account_label:?}: {} failed: {error}", *uri)
          });
          assert_eq!(parsed, original, "{issuer:?} / {account_label:?}");
          checked += 1;
        }
      }
    }

    assert!(checked > 7_000, "only {checked} labels were valid");
  }
}
