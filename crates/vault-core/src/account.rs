//! [`AccountKey`]: when two entries are the same account.

use std::hash::Hash;

use crate::{Entry, OtpConfig, SecretBytes};

/// What makes two accounts the same one: issuer and account name (in any
/// letter case, ignoring surrounding whitespace), OTP scheme and secret.
///
/// Two accounts that differ only in secret are different enrolments, and
/// both are kept. A missing issuer is the same as an empty one.
///
/// Import uses this to skip accounts the vault already has, and sync to
/// collapse the copies of an account that two devices added separately.
/// It is an in-memory key only: it holds the secret, and must never be
/// written anywhere. `Debug` is safe to derive: [`SecretBytes`] redacts
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountKey {
  issuer: String,
  account_label: String,
  kind: OtpKind,
  secret: SecretBytes,
}

/// The OTP scheme, without its parameters: an account keeps its identity
/// when, say, its period is corrected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OtpKind {
  Totp,
  Hotp,
}

impl AccountKey {
  #[must_use]
  pub fn new(issuer: Option<&str>, account_label: &str, otp: &OtpConfig) -> Self {
    let (kind, secret) = match otp {
      OtpConfig::Totp { secret, .. } => (OtpKind::Totp, secret),
      OtpConfig::Hotp { secret, .. } => (OtpKind::Hotp, secret),
    };
    Self {
      issuer: normalize(issuer.unwrap_or_default()),
      account_label: normalize(account_label),
      kind,
      secret: secret.clone(),
    }
  }
}

impl From<&Entry> for AccountKey {
  fn from(entry: &Entry) -> Self {
    Self::new(entry.issuer.as_deref(), &entry.account_label, &entry.otp)
  }
}

fn normalize(text: &str) -> String {
  text.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
  use otp::Algorithm;

  use super::*;

  fn totp(secret: &[u8], period: u64) -> OtpConfig {
    OtpConfig::Totp {
      secret: secret.to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      period,
    }
  }

  fn hotp(secret: &[u8]) -> OtpConfig {
    OtpConfig::Hotp {
      secret: secret.to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      counter: 0,
    }
  }

  #[test]
  fn letter_case_and_surrounding_spaces_do_not_matter() {
    assert_eq!(
      AccountKey::new(Some("GitHub"), "Octocat", &totp(b"seed", 30)),
      AccountKey::new(Some("  github "), "octocat ", &totp(b"seed", 30)),
    );
  }

  #[test]
  fn a_missing_issuer_is_an_empty_one() {
    assert_eq!(
      AccountKey::new(None, "alice", &totp(b"seed", 30)),
      AccountKey::new(Some(" "), "alice", &totp(b"seed", 30)),
    );
  }

  #[test]
  fn a_different_secret_is_a_different_account() {
    assert_ne!(
      AccountKey::new(None, "alice", &totp(b"seed-1", 30)),
      AccountKey::new(None, "alice", &totp(b"seed-2", 30)),
    );
  }

  #[test]
  fn a_different_scheme_is_a_different_account() {
    assert_ne!(
      AccountKey::new(None, "alice", &totp(b"seed", 30)),
      AccountKey::new(None, "alice", &hotp(b"seed")),
    );
  }

  #[test]
  fn parameters_other_than_the_scheme_do_not_matter() {
    assert_eq!(
      AccountKey::new(None, "alice", &totp(b"seed", 30)),
      AccountKey::new(None, "alice", &totp(b"seed", 60)),
    );
  }

  #[test]
  fn different_names_are_different_accounts() {
    assert_ne!(
      AccountKey::new(Some("A"), "alice", &totp(b"seed", 30)),
      AccountKey::new(Some("B"), "alice", &totp(b"seed", 30)),
    );
    assert_ne!(
      AccountKey::new(None, "alice", &totp(b"seed", 30)),
      AccountKey::new(None, "bob", &totp(b"seed", 30)),
    );
  }
}
