//! TOTP (RFC 6238) and HOTP (RFC 4226) code generation.
//!
//! There is exactly one implementation of the algorithm here: HOTP is
//! `Truncate(HMAC(K, C))` (RFC 4226 §5.3), and TOTP is by definition
//! HOTP with the counter replaced by the current time step,
//! `floor(unix_time / period)` (RFC 6238 §4). Both public entry points,
//! [`hotp`] and [`Totp`], validate their input and then call the same
//! private generator, so a change to truncation or digit handling can
//! never make the two disagree.
//!
//! Secrets are always *borrowed*: nothing in this crate copies a seed,
//! so callers keep full control over where secret bytes live and when
//! they are zeroized.

use hmac::{Hmac, KeyInit, Mac, digest::block_api::EagerHash};
use sha1::Sha1;
use sha2::{Sha256, Sha512};

/// Hash algorithm for OTP generation. SHA1 is the de-facto universal
/// default — it's what RFC 4226 specifies and what RFC 6238 uses as its
/// reference case. SHA256/SHA512 are permitted by RFC 6238 but many
/// real-world verifiers ignore the algorithm parameter entirely and
/// assume SHA1, so support them but don't expect wide interop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Algorithm {
  Sha1,
  Sha256,
  Sha512,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OtpError {
  #[error("digit count must be 6, 7, or 8 per RFC 4226 §5.3 — got {0}")]
  InvalidDigits(u8),
  #[error("period must be nonzero and at most {max}s — got {got}")]
  InvalidPeriod { got: u64, max: u64 },
  #[error("secret must not be empty")]
  EmptySecret,
  /// The HOTP counter is at `u64::MAX`: a code could be generated, but
  /// the counter could never advance past it, so the entry is unusable.
  #[error("HOTP counter is exhausted and cannot advance")]
  CounterExhausted,
}

/// Generous upper bound on TOTP period. Real-world periods are 30s
/// (occasionally 60s); anything beyond this from a scanned QR/URI is
/// almost certainly malformed or malicious input, not a real service.
const MAX_PERIOD_SECS: u64 = 300;

fn validate_digits(digits: u8) -> Result<(), OtpError> {
  if (6..=8).contains(&digits) {
    Ok(())
  } else {
    Err(OtpError::InvalidDigits(digits))
  }
}

/// RFC 4226 §4's "R6" requires generators to use a secret of at least
/// 128 bits — but that requirement binds whoever *issues* a secret, not
/// whoever consumes one. This app never generates TOTP/HOTP secrets, it
/// only ever imports ones an external service already created, and a
/// meaningful number of real services (including Google's own
/// `google-authenticator` PAM module, historically — see
/// github.com/google/google-authenticator/issues/339, and Google's own
/// published otpauth spec example, which decodes to 10 bytes) still
/// use 80-bit secrets. HMAC itself (RFC 2104) accepts a key of any
/// length, so enforcing the 128-bit floor here would only break
/// importing real, already-deployed accounts — it wouldn't make any of
/// them more secure, since this app has no control over what the
/// issuing service already chose. The only thing actually invalid is
/// an empty secret, which is a real error (e.g. a parsing bug upstream)
/// rather than a weak-but-real one.
fn validate_secret(secret: &[u8]) -> Result<(), OtpError> {
  if secret.is_empty() {
    Err(OtpError::EmptySecret)
  } else {
    Ok(())
  }
}

fn validate_period(period: u64) -> Result<(), OtpError> {
  if period > 0 && period <= MAX_PERIOD_SECS {
    Ok(())
  } else {
    Err(OtpError::InvalidPeriod {
      got: period,
      max: MAX_PERIOD_SECS,
    })
  }
}

/// The stored HOTP counter is the *next* one to use, so a counter that
/// cannot be incremented can never be used safely.
fn validate_counter(counter: u64) -> Result<(), OtpError> {
  if counter == u64::MAX {
    Err(OtpError::CounterExhausted)
  } else {
    Ok(())
  }
}

/// RFC 4226 §5.3 dynamic truncation: the 31-bit value at the offset
/// named by the low nibble of the last byte.
fn dynamic_truncate(mac: &[u8]) -> u32 {
  let offset = usize::from(mac[mac.len() - 1] & 0x0f);
  let bytes = [
    mac[offset],
    mac[offset + 1],
    mac[offset + 2],
    mac[offset + 3],
  ];
  u32::from_be_bytes(bytes) & 0x7fff_ffff
}

fn truncated_hmac<D: EagerHash>(secret: &[u8], counter: u64) -> u32
where
  Hmac<D>: KeyInit + Mac,
{
  // HMAC accepts a key of any length (RFC 2104); `new_from_slice` only
  // fails for fixed-key-size MACs, which HMAC is not.
  let mut mac = <Hmac<D> as KeyInit>::new_from_slice(secret).expect("HMAC accepts any key length");
  mac.update(&counter.to_be_bytes());
  dynamic_truncate(&mac.finalize().into_bytes())
}

/// `HOTP(K, C) = Truncate(HMAC(K, C)) mod 10^digits`, zero-padded.
/// Callers have already validated `digits` and `secret`.
fn generate(secret: &[u8], counter: u64, algorithm: Algorithm, digits: u8) -> String {
  let value = match algorithm {
    Algorithm::Sha1 => truncated_hmac::<Sha1>(secret, counter),
    Algorithm::Sha256 => truncated_hmac::<Sha256>(secret, counter),
    Algorithm::Sha512 => truncated_hmac::<Sha512>(secret, counter),
  };
  let code = value % 10u32.pow(u32::from(digits));
  format!("{code:0width$}", width = usize::from(digits))
}

/// Compute an HOTP code per RFC 4226: `HOTP(K, C) = Truncate(HMAC(K, C))`.
///
/// Doubles as the validation every HOTP account goes through before it
/// is stored (the `qr` and `interop` parsers call it), so it also
/// rejects a counter that could never be advanced.
///
/// # Errors
///
/// Returns [`OtpError::InvalidDigits`] if `digits` is not 6, 7 or 8,
/// [`OtpError::EmptySecret`] if `secret` is empty, or
/// [`OtpError::CounterExhausted`] if `counter` is `u64::MAX`.
pub fn hotp(
  secret: &[u8],
  counter: u64,
  algorithm: Algorithm,
  digits: u8,
) -> Result<String, OtpError> {
  validate_digits(digits)?;
  validate_secret(secret)?;
  validate_counter(counter)?;

  Ok(generate(secret, counter, algorithm, digits))
}

/// A validated TOTP generator per RFC 6238, borrowing its secret.
///
/// No `Debug` impl: it holds a reference to secret key material.
pub struct Totp<'secret> {
  secret: &'secret [u8],
  algorithm: Algorithm,
  digits: u8,
  period: u64,
}

impl<'secret> Totp<'secret> {
  /// # Errors
  ///
  /// Returns [`OtpError::InvalidDigits`], [`OtpError::EmptySecret`] or
  /// [`OtpError::InvalidPeriod`] if the corresponding input is invalid.
  pub fn new(
    secret: &'secret [u8],
    algorithm: Algorithm,
    digits: u8,
    period: u64,
  ) -> Result<Self, OtpError> {
    validate_digits(digits)?;
    validate_secret(secret)?;
    validate_period(period)?;

    Ok(Self {
      secret,
      algorithm,
      digits,
      period,
    })
  }

  /// The time step length, in seconds.
  #[must_use]
  pub const fn period(&self) -> u64 {
    self.period
  }

  /// The code for the time step containing `unix_time` (seconds).
  #[must_use]
  pub fn generate(&self, unix_time: u64) -> String {
    self.code_for_step(unix_time / self.period)
  }

  /// The code for the time step *after* the one containing `unix_time`
  /// — what [`Self::generate`] will return once
  /// [`Self::seconds_remaining`] runs out.
  #[must_use]
  pub fn next_code(&self, unix_time: u64) -> String {
    // Saturates only at the end of `u64` time, where no next step exists.
    self.code_for_step((unix_time / self.period).saturating_add(1))
  }

  /// Seconds remaining before the current code rotates (1..=period).
  #[must_use]
  pub fn seconds_remaining(&self, unix_time: u64) -> u64 {
    self.period - unix_time % self.period
  }

  fn code_for_step(&self, step: u64) -> String {
    generate(self.secret, step, self.algorithm, self.digits)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // ---- RFC 4226 Appendix D — HOTP test vectors -----------------------
  // Secret is the ASCII string "12345678901234567890" (20 bytes),
  // SHA1, 6 digits. Verified directly against rfc-editor.org/rfc/rfc4226
  // and cross-checked against two independent third-party test suites.

  const RFC4226_SECRET: &[u8] = b"12345678901234567890";
  const RFC4226_EXPECTED: [&str; 10] = [
    "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871",
    "520489",
  ];

  #[test]
  fn rfc4226_hotp_vectors() {
    for (counter, expected) in (0u64..).zip(RFC4226_EXPECTED) {
      let code = hotp(RFC4226_SECRET, counter, Algorithm::Sha1, 6).unwrap();
      assert_eq!(code, expected, "counter {counter}");
    }
  }

  /// RFC 4226 Appendix D's intermediate values: the truncated 31-bit
  /// integer before `mod 10^digits`, which exercises
  /// `dynamic_truncate` independently of digit handling.
  #[test]
  fn rfc4226_truncated_values() {
    let expected: [u32; 10] = [
      1_284_755_224,
      1_094_287_082,
      137_359_152,
      1_726_969_429,
      1_640_338_314,
      868_254_676,
      1_918_287_922,
      82_162_583,
      673_399_871,
      645_520_489,
    ];
    for (counter, value) in (0u64..).zip(expected) {
      assert_eq!(
        truncated_hmac::<Sha1>(RFC4226_SECRET, counter),
        value,
        "counter {counter}"
      );
    }
  }

  #[test]
  fn hotp_rejects_empty_secret() {
    let err = hotp(b"", 0, Algorithm::Sha1, 6).unwrap_err();
    assert_eq!(err, OtpError::EmptySecret);
  }

  #[test]
  fn hotp_accepts_a_secret_shorter_than_the_rfc_recommendation() {
    // RFC 4226's 128-bit floor binds generators, not consumers — see
    // the doc comment on `validate_secret`. A real service's
    // 80-bit secret (Google's own published otpauth example is
    // exactly this length) must still work.
    let short_secret = b"01234567890"; // 11 bytes, well under the 16-byte RFC recommendation
    assert!(hotp(short_secret, 0, Algorithm::Sha1, 6).is_ok());
  }

  #[test]
  fn hotp_rejects_bad_digit_count() {
    assert_eq!(
      hotp(RFC4226_SECRET, 0, Algorithm::Sha1, 5).unwrap_err(),
      OtpError::InvalidDigits(5)
    );
    assert_eq!(
      hotp(RFC4226_SECRET, 0, Algorithm::Sha1, 9).unwrap_err(),
      OtpError::InvalidDigits(9)
    );
  }

  #[test]
  fn hotp_rejects_a_counter_that_cannot_advance() {
    assert_eq!(
      hotp(RFC4226_SECRET, u64::MAX, Algorithm::Sha1, 6),
      Err(OtpError::CounterExhausted)
    );
    assert!(hotp(RFC4226_SECRET, u64::MAX - 1, Algorithm::Sha1, 6).is_ok());
  }

  #[test]
  fn hotp_pads_codes_with_leading_zeros() {
    // RFC 6238 Appendix B, SHA1 at T=0x23523EC: "07081804".
    assert_eq!(
      hotp(RFC4226_SECRET, 0x0235_23EC, Algorithm::Sha1, 8).unwrap(),
      "07081804"
    );
  }

  // ---- RFC 6238 Appendix B — TOTP test vectors -----------------------
  // Secrets are built the same way the RFC's own reference
  // implementation builds them: repeat/extend the 20-byte SHA1 seed to
  // the target length. 8 digits, 30s period, per the RFC table.
  // Verified directly against rfc-editor.org/rfc/rfc6238.

  fn seed_sha256() -> Vec<u8> {
    RFC4226_SECRET.iter().cycle().take(32).copied().collect()
  }

  fn seed_sha512() -> Vec<u8> {
    RFC4226_SECRET.iter().cycle().take(64).copied().collect()
  }

  const RFC6238_TIMES: [u64; 6] = [
    59,
    1_111_111_109,
    1_111_111_111,
    1_234_567_890,
    2_000_000_000,
    20_000_000_000,
  ];

  fn assert_rfc6238(totp: &Totp<'_>, expected: [&str; 6]) {
    for (time, code) in RFC6238_TIMES.iter().zip(expected) {
      assert_eq!(totp.generate(*time), code, "time {time}");
    }
  }

  #[test]
  fn rfc6238_totp_vectors_sha1() {
    let totp = Totp::new(RFC4226_SECRET, Algorithm::Sha1, 8, 30).unwrap();
    assert_rfc6238(
      &totp,
      [
        "94287082", "07081804", "14050471", "89005924", "69279037", "65353130",
      ],
    );
  }

  #[test]
  fn rfc6238_totp_vectors_sha256() {
    let seed = seed_sha256();
    let totp = Totp::new(&seed, Algorithm::Sha256, 8, 30).unwrap();
    assert_rfc6238(
      &totp,
      [
        "46119246", "68084774", "67062674", "91819424", "90698825", "77737706",
      ],
    );
  }

  #[test]
  fn rfc6238_totp_vectors_sha512() {
    let seed = seed_sha512();
    let totp = Totp::new(&seed, Algorithm::Sha512, 8, 30).unwrap();
    assert_rfc6238(
      &totp,
      [
        "90693936", "25091201", "99943326", "93441116", "38618901", "47863826",
      ],
    );
  }

  #[test]
  fn totp_is_hotp_of_the_time_step() {
    let totp = Totp::new(RFC4226_SECRET, Algorithm::Sha1, 6, 30).unwrap();
    for time in [0, 29, 30, 59, 1_234_567_890] {
      assert_eq!(
        totp.generate(time),
        hotp(RFC4226_SECRET, time / 30, Algorithm::Sha1, 6).unwrap()
      );
    }
  }

  #[test]
  fn next_code_is_the_code_of_the_following_step() {
    let totp = Totp::new(RFC4226_SECRET, Algorithm::Sha1, 8, 30).unwrap();
    // RFC 6238 vectors at T=59 (step 1) and T=1111111109/1111111111,
    // which straddle a step boundary.
    assert_eq!(totp.next_code(29), totp.generate(59));
    assert_eq!(totp.next_code(1_111_111_109), "14050471");
    assert_eq!(totp.next_code(1_111_111_109), totp.generate(1_111_111_111));
  }

  #[test]
  fn next_code_does_not_overflow_at_the_end_of_time() {
    let totp = Totp::new(RFC4226_SECRET, Algorithm::Sha1, 6, 1).unwrap();
    assert_eq!(totp.next_code(u64::MAX), totp.generate(u64::MAX));
  }

  #[test]
  fn totp_honors_non_default_periods() {
    let totp = Totp::new(RFC4226_SECRET, Algorithm::Sha1, 6, 60).unwrap();
    assert_eq!(totp.period(), 60);
    assert_eq!(totp.generate(59), totp.generate(0));
    assert_ne!(totp.generate(60), totp.generate(59));
    assert_eq!(totp.seconds_remaining(0), 60);
    assert_eq!(totp.seconds_remaining(59), 1);
    assert_eq!(totp.seconds_remaining(60), 60);
  }

  #[test]
  fn totp_rejects_zero_period() {
    assert!(matches!(
      Totp::new(RFC4226_SECRET, Algorithm::Sha1, 6, 0),
      Err(OtpError::InvalidPeriod { got: 0, .. })
    ));
  }

  #[test]
  fn totp_rejects_absurd_period() {
    // Guards against a malformed/malicious otpauth:// URI claiming
    // an hours-long period.
    assert!(Totp::new(RFC4226_SECRET, Algorithm::Sha1, 6, 999_999).is_err());
  }

  #[test]
  fn totp_rejects_empty_secret_and_bad_digits() {
    assert!(matches!(
      Totp::new(b"", Algorithm::Sha1, 6, 30),
      Err(OtpError::EmptySecret)
    ));
    assert!(matches!(
      Totp::new(RFC4226_SECRET, Algorithm::Sha1, 10, 30),
      Err(OtpError::InvalidDigits(10))
    ));
  }
}
