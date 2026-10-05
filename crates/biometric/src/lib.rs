//! Biometric unlock on Android: the vault key, sealed under an Android
//! Keystore key that the secure hardware uses only after a strong
//! (class 3) biometric check — a fingerprint, or a face on phones whose
//! face unlock is that strong.
//!
//! This is not a yes/no "the user is authenticated" check, which a
//! compromised app process could fake: the hardware itself refuses to
//! decrypt until the biometric check passes, so the sealed vault key is
//! useless without the user's finger or face, even to whoever copies the
//! app's files. Adding a fingerprint or face to the phone destroys the
//! Keystore key, and with it every sealed vault key.
//!
//! Driven from Rust only: the plugin has no commands, and the `WebView`
//! never sees a key.

#![cfg(target_os = "android")]

use base64::{Engine, prelude::BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use tauri::{
  Manager, Runtime,
  plugin::{Builder, PluginHandle, TauriPlugin, mobile::PluginInvokeError},
};
use zeroize::Zeroizing;

const PLUGIN_IDENTIFIER: &str = "io.github.amad3v.arsu.biometric";

/// The text of the system's biometric prompt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
  pub title: String,
  pub subtitle: String,
  /// The prompt's button that gives up, e.g. "Use password".
  pub cancel: String,
}

/// Whether biometric unlock can be used on this phone.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
  /// A strong biometric is enrolled and usable.
  pub available: bool,
  /// Why not, when it isn't: `no-hardware`, `not-enrolled`,
  /// `unavailable`, `update-required` or `unsupported`.
  pub reason: Option<String>,
  /// The Keystore key exists, so something may be sealed under it.
  pub key_exists: bool,
}

/// Data sealed under the Keystore key: AES-256-GCM, its IV and its
/// ciphertext (with the tag). Not secret without the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
  pub iv: Vec<u8>,
  pub ciphertext: Vec<u8>,
}

/// Why sealing or unsealing failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// The user dismissed the prompt or chose the password instead.
  #[error("biometric authentication was cancelled")]
  Cancelled,
  /// Too many failed attempts: biometrics are locked for a while, or
  /// until the phone is unlocked with its PIN.
  #[error("too many failed attempts; biometrics are locked for now")]
  Lockout,
  /// The Keystore key is gone or was invalidated (a fingerprint or face
  /// was added or removed): what was sealed under it is lost for good.
  #[error(
    "the biometric key no longer exists; it is invalidated when the phone's fingerprints or faces change"
  )]
  Invalidated,
  /// No strong biometric is set up, or the hardware cannot be used now.
  #[error("biometric authentication is not available on this phone")]
  Unavailable,
  #[error("biometric authentication failed: {0}")]
  Failed(String),
}

impl From<PluginInvokeError> for Error {
  fn from(error: PluginInvokeError) -> Self {
    match error {
      PluginInvokeError::InvokeRejected(response) => match response.code.as_deref() {
        Some("cancelled") => Self::Cancelled,
        Some("lockout") => Self::Lockout,
        Some("invalidated") => Self::Invalidated,
        Some("unavailable") => Self::Unavailable,
        _ => Self::Failed(response.message.unwrap_or_default()),
      },
      other => Self::Failed(other.to_string()),
    }
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealArgs<'a> {
  #[serde(flatten)]
  prompt: &'a Prompt,
  data: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenArgs<'a> {
  #[serde(flatten)]
  prompt: &'a Prompt,
  iv: String,
  ciphertext: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SealedWire {
  iv: String,
  ciphertext: String,
}

#[derive(Deserialize)]
struct OpenedWire {
  data: String,
}

fn decode(field: &str) -> Result<Vec<u8>, Error> {
  BASE64_STANDARD
    .decode(field)
    .map_err(|error| Error::Failed(format!("malformed reply from the plugin: {error}")))
}

/// The plugin, as the app manages it.
pub struct Biometric<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Biometric<R> {
  /// Whether a strong biometric can be used, and whether the Keystore key
  /// exists.
  ///
  /// # Errors
  ///
  /// [`Error::Failed`] if the plugin cannot be reached.
  pub fn status(&self) -> Result<Status, Error> {
    Ok(self.0.run_mobile_plugin("status", ())?)
  }

  /// Creates a new Keystore key (replacing any earlier one), shows the
  /// biometric prompt, and seals `data` under the key once it passes.
  /// Blocks until the user answers: never call it on the main thread.
  ///
  /// # Errors
  ///
  /// [`Error::Cancelled`], [`Error::Lockout`], [`Error::Unavailable`] or
  /// [`Error::Failed`].
  pub fn seal(&self, prompt: &Prompt, data: &[u8]) -> Result<Sealed, Error> {
    let data = Zeroizing::new(BASE64_STANDARD.encode(data));
    let sealed: SealedWire = self.0.run_mobile_plugin(
      "seal",
      SealArgs {
        prompt,
        data: &data,
      },
    )?;
    Ok(Sealed {
      iv: decode(&sealed.iv)?,
      ciphertext: decode(&sealed.ciphertext)?,
    })
  }

  /// Shows the biometric prompt and unseals `sealed` once it passes.
  /// Blocks until the user answers: never call it on the main thread.
  ///
  /// # Errors
  ///
  /// [`Error::Invalidated`] if the key is gone (the sealed data then is
  /// too), [`Error::Cancelled`], [`Error::Lockout`],
  /// [`Error::Unavailable`] or [`Error::Failed`].
  pub fn open(&self, prompt: &Prompt, sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>, Error> {
    let opened: OpenedWire = self.0.run_mobile_plugin(
      "open",
      OpenArgs {
        prompt,
        iv: BASE64_STANDARD.encode(&sealed.iv),
        ciphertext: BASE64_STANDARD.encode(&sealed.ciphertext),
      },
    )?;
    let data = Zeroizing::new(opened.data);
    Ok(Zeroizing::new(decode(&data)?))
  }

  /// Deletes the Keystore key, so that nothing sealed under it can be
  /// unsealed again.
  ///
  /// # Errors
  ///
  /// [`Error::Failed`] if the plugin cannot be reached.
  pub fn forget(&self) -> Result<(), Error> {
    Ok(self.0.run_mobile_plugin("forget", ())?)
  }
}

/// `app.biometric()` reaches the plugin.
pub trait BiometricExt<R: Runtime> {
  fn biometric(&self) -> &Biometric<R>;
}

impl<R: Runtime, T: Manager<R>> BiometricExt<R> for T {
  fn biometric(&self) -> &Biometric<R> {
    self.state::<Biometric<R>>().inner()
  }
}

/// The plugin, to register with the app's builder.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("arsu-biometric")
    .setup(|app, api| {
      let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "BiometricPlugin")?;
      app.manage(Biometric(handle));
      Ok(())
    })
    .build()
}
