//! Biometric unlock: the vault key, sealed by the platform under a key
//! that only a strong biometric check releases (on Android, the
//! `biometric` plugin over Android Keystore), so the vault opens with a
//! fingerprint or a face instead of the master password.
//!
//! What is sealed is the vault key itself, never the password, and what
//! is stored is only its sealed form (see `state/biometric.rs`): useless
//! without the hardware-held key, which the phone deletes when its
//! fingerprints or faces change. The master password stays the way in,
//! and the only way to turn biometric unlock on.
//!
//! [`KeySealer`] is the platform's side, kept behind a trait so that the
//! vault side is tested on any platform.

use zeroize::Zeroizing;

/// Data sealed by a [`KeySealer`]: an IV and a ciphertext. Not secret
/// without the sealer's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
  pub iv: Vec<u8>,
  pub ciphertext: Vec<u8>,
}

/// Why sealing or unsealing failed.
#[derive(Debug, thiserror::Error)]
pub enum SealError {
  /// The user dismissed the prompt, or chose the password instead.
  #[error("biometric authentication was cancelled")]
  Cancelled,
  /// Too many failed attempts: biometrics are locked for a while.
  #[error("too many failed attempts; biometrics are locked for now")]
  Lockout,
  /// The sealing key is gone: the phone's fingerprints or faces changed.
  /// Whatever it sealed is lost.
  #[error("biometric unlock was turned off because this phone's fingerprints or faces changed")]
  Invalidated,
  /// No strong biometric is set up, or this platform has none.
  #[error("biometric unlock is not available on this device")]
  Unavailable,
  #[error("biometric authentication failed: {0}")]
  Failed(String),
}

/// What the sealed key is opened for: the system prompt says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsealFor {
  /// Unlocking the vault.
  Unlock,
  /// Showing an entry's secret, as a QR code, in the unlocked vault.
  ShowSecret,
}

/// The platform's sealing: each call asks the user for their biometric.
pub trait KeySealer {
  /// Seals `secret` under a new key, replacing any earlier one.
  ///
  /// # Errors
  ///
  /// Returns why the user or the platform refused.
  fn seal(&self, secret: &[u8]) -> Result<Sealed, SealError>;

  /// Unseals what [`Self::seal`] sealed, asking the user's biometric
  /// for `purpose`.
  ///
  /// # Errors
  ///
  /// [`SealError::Invalidated`] if the key is gone, or why the user or the
  /// platform refused.
  fn open(&self, sealed: &Sealed, purpose: UnsealFor) -> Result<Zeroizing<Vec<u8>>, SealError>;

  /// Deletes the key, so nothing it sealed opens again.
  ///
  /// # Errors
  ///
  /// Returns why the platform refused.
  fn forget(&self) -> Result<(), SealError>;
}

/// What the platform offers, for the settings and the unlock screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
  /// The platform has biometric unlock at all (Android).
  pub supported: bool,
  /// A strong biometric is set up and usable now.
  pub available: bool,
  /// Why not, when not: `no-hardware`, `not-enrolled`, `unavailable`,
  /// `update-required` or `unsupported`.
  pub reason: Option<String>,
}

#[cfg(target_os = "android")]
pub use android::{AndroidSealer as PlatformSealer, availability};

#[cfg(not(target_os = "android"))]
pub use desktop::{NoSealer as PlatformSealer, availability};

#[cfg(target_os = "android")]
mod android {
  use biometric::{BiometricExt, Error, Prompt};
  use tauri::{AppHandle, Runtime};
  use zeroize::Zeroizing;

  use super::{Availability, KeySealer, SealError, Sealed, UnsealFor};

  impl From<Error> for SealError {
    fn from(error: Error) -> Self {
      match error {
        Error::Cancelled => Self::Cancelled,
        Error::Lockout => Self::Lockout,
        Error::Invalidated => Self::Invalidated,
        Error::Unavailable => Self::Unavailable,
        Error::Failed(message) => Self::Failed(message),
      }
    }
  }

  /// Android Keystore, through the `biometric` plugin.
  pub struct AndroidSealer<R: Runtime>(pub AppHandle<R>);

  impl<R: Runtime> AndroidSealer<R> {
    #[must_use]
    pub fn new(app: &AppHandle<R>) -> Self {
      Self(app.clone())
    }
  }

  impl<R: Runtime> KeySealer for AndroidSealer<R> {
    fn seal(&self, secret: &[u8]) -> Result<Sealed, SealError> {
      let prompt = Prompt {
        title: "Turn on biometric unlock".to_owned(),
        subtitle: "Confirm with your fingerprint or face".to_owned(),
        cancel: "Cancel".to_owned(),
      };
      let sealed = self.0.biometric().seal(&prompt, secret)?;
      Ok(Sealed {
        iv: sealed.iv,
        ciphertext: sealed.ciphertext,
      })
    }

    fn open(&self, sealed: &Sealed, purpose: UnsealFor) -> Result<Zeroizing<Vec<u8>>, SealError> {
      let prompt = match purpose {
        UnsealFor::Unlock => Prompt {
          title: "Unlock Arsu".to_owned(),
          subtitle: "Use your fingerprint or face".to_owned(),
          cancel: "Use password".to_owned(),
        },
        UnsealFor::ShowSecret => Prompt {
          title: "Show QR code".to_owned(),
          subtitle: "Confirm with your fingerprint or face".to_owned(),
          cancel: "Use password".to_owned(),
        },
      };
      let sealed = biometric::Sealed {
        iv: sealed.iv.clone(),
        ciphertext: sealed.ciphertext.clone(),
      };
      Ok(self.0.biometric().open(&prompt, &sealed)?)
    }

    fn forget(&self) -> Result<(), SealError> {
      Ok(self.0.biometric().forget()?)
    }
  }

  /// Whether the phone can unlock with a strong biometric now.
  pub fn availability<R: Runtime>(app: &AppHandle<R>) -> Availability {
    match app.biometric().status() {
      Ok(status) => Availability {
        supported: true,
        available: status.available,
        reason: status.reason,
      },
      Err(_) => Availability {
        supported: true,
        available: false,
        reason: Some("unavailable".to_owned()),
      },
    }
  }
}

#[cfg(not(target_os = "android"))]
mod desktop {
  use tauri::{AppHandle, Runtime};
  use zeroize::Zeroizing;

  use super::{Availability, KeySealer, SealError, Sealed, UnsealFor};

  /// The Linux desktop has no biometric unlock: every call refuses.
  pub struct NoSealer;

  impl NoSealer {
    #[must_use]
    pub fn new<R: Runtime>(_app: &AppHandle<R>) -> Self {
      Self
    }
  }

  impl KeySealer for NoSealer {
    fn seal(&self, _secret: &[u8]) -> Result<Sealed, SealError> {
      Err(SealError::Unavailable)
    }

    fn open(&self, _sealed: &Sealed, _purpose: UnsealFor) -> Result<Zeroizing<Vec<u8>>, SealError> {
      Err(SealError::Unavailable)
    }

    fn forget(&self) -> Result<(), SealError> {
      Ok(())
    }
  }

  /// Never available here.
  #[must_use]
  pub fn availability<R: Runtime>(_app: &AppHandle<R>) -> Availability {
    Availability {
      supported: false,
      available: false,
      reason: Some("unsupported".to_owned()),
    }
  }
}
