//! OTP accounts from untrusted text: `otpauth://` URIs (typed, pasted
//! or read from a QR code) and manual entry, plus the validated
//! [`ParsedAccount`] type and the decoding rules the `interop` importers
//! share with them.
//!
//! Every input here is treated as attacker-controlled, not merely
//! malformed by accident: nothing in this crate panics on bad input, and
//! every failure is a [`QrError`].

mod account;
mod uri;

use qrcode::{QrCode, render::svg::Color as QrColor};

pub use account::{
  DEFAULT_ALGORITHM, DEFAULT_DIGITS, DEFAULT_PERIOD_SECS, ManualEntryInput, OtpKind, OtpParams,
  ParsedAccount, algorithm_name, decode_secret, encode_secret, parse_algorithm, parse_manual_entry,
};
pub use uri::{build_otpauth_uri, parse_otpauth_uri};

/// Why an account was rejected. The messages describe the problem
/// without naming a source format, because the importers report them as
/// skip reasons too.
#[derive(Debug, thiserror::Error)]
pub enum QrError {
  #[error("not an otpauth:// URI")]
  NotOtpauth,
  #[error("the account has no name")]
  MissingLabel,
  #[error("the issuer and account name must not contain ':'")]
  LabelContainsColon,
  #[error("the label is not valid UTF-8 once percent-decoded")]
  InvalidLabelEncoding,
  #[error("the secret is missing")]
  MissingSecret,
  #[error("the secret is not valid base32")]
  InvalidSecret,
  #[error("the HOTP counter is missing")]
  MissingCounter,
  #[error("'{0}' is not a valid number")]
  InvalidNumber(&'static str),
  #[error("unrecognized OTP type '{0}' (expected TOTP or HOTP)")]
  UnknownType(String),
  #[error("unrecognized algorithm '{0}' (expected SHA1, SHA256 or SHA512)")]
  UnknownAlgorithm(String),
  #[error(transparent)]
  InvalidOtpConfig(#[from] otp::OtpError),
  #[error("failed to encode the QR code")]
  QrEncode(#[source] qrcode::types::QrError),
}

/// Renders an `otpauth://` URI as a scannable QR code, returned as an
/// SVG string: the frontend displays SVG natively, so encoding needs no
/// raster image dependency.
///
/// # Errors
///
/// Returns [`QrError::QrEncode`] if the URI cannot be encoded as a QR
/// code (it is too long).
pub fn encode_qr_svg(otpauth_uri: &str) -> Result<String, QrError> {
  let code = QrCode::new(otpauth_uri.as_bytes()).map_err(QrError::QrEncode)?;

  Ok(
    code
      .render()
      .min_dimensions(300, 300)
      .dark_color(QrColor("#000000"))
      .light_color(QrColor("#ffffff"))
      .build(),
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn encode_qr_svg_produces_valid_looking_svg() {
    let svg = encode_qr_svg(
      "otpauth://totp/Example:alice@google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example",
    )
    .unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.len() > 100, "suspiciously short SVG output");
  }

  #[test]
  fn encode_qr_svg_rejects_data_too_long_for_a_qr_code() {
    assert!(matches!(
      encode_qr_svg(&"A".repeat(10_000)),
      Err(QrError::QrEncode(_))
    ));
  }
}
