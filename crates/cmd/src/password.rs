//! The minimum strength for passwords that protect a copy of every seed:
//! the master password of a new vault, and the password of an encrypted
//! export.
//!
//! Argon2id and scrypt make each guess expensive, but no key derivation
//! makes `1234` safe. The policy is a length floor, as NIST SP 800-63B
//! recommends over composition rules; it applies when a password is
//! chosen, never to unlocking, so vaults created before it keep opening.

use crate::{dto::SecretString, error::AppError};

/// The minimum length of a new password, in Unicode scalar values.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// # Errors
///
/// Returns [`AppError::WeakPassword`] if `password` is shorter than
/// [`MIN_PASSWORD_CHARS`].
pub fn ensure_strong(password: &SecretString) -> Result<(), AppError> {
  if password.expose().chars().count() >= MIN_PASSWORD_CHARS {
    Ok(())
  } else {
    Err(AppError::WeakPassword)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn check(password: &str) -> Result<(), AppError> {
    ensure_strong(&SecretString::from(password.to_owned()))
  }

  #[test]
  fn passwords_need_twelve_characters() {
    assert!(matches!(check(""), Err(AppError::WeakPassword)));
    assert!(matches!(check("elevenchars"), Err(AppError::WeakPassword)));
    assert!(check("twelve chars").is_ok());
  }

  #[test]
  fn length_counts_characters_not_bytes() {
    // 6 characters, 12 bytes in UTF-8.
    assert!(matches!(check("éééééé"), Err(AppError::WeakPassword)));
    assert!(check("éééééééééééé").is_ok());
  }
}
