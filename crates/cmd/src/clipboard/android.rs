//! The Android clipboard, through the framework's `ClipboardManager`.

use super::SystemClipboard;
use crate::android::{self, JavaError};

/// What can go wrong reading, writing or clearing the system clipboard.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
  /// The clipboard holds no text, or the app may not read it (it is not
  /// in the foreground).
  #[error("the clipboard holds no text this app can read")]
  Unreadable,
  #[error(transparent)]
  Operation(#[from] JavaError),
}

/// The system clipboard. Holds nothing itself: every operation goes to
/// the clipboard service, which keeps the clip after the app exits.
#[derive(Debug, Default)]
pub struct Clipboard;

impl Clipboard {
  #[must_use]
  pub fn new() -> Self {
    Self
  }

  /// Nothing to release on Android.
  pub fn close(&self) {}
}

impl SystemClipboard for Clipboard {
  type Error = ClipboardError;

  /// See the module docs of [`super`]: a code the user pasted into
  /// another app is still cleared after the delay.
  const CLEAR_IF_UNREADABLE: bool = true;

  fn read_text(&self) -> Result<String, Self::Error> {
    android::clipboard_text()?.ok_or(ClipboardError::Unreadable)
  }

  fn write_text(&self, text: &str) -> Result<(), Self::Error> {
    Ok(android::set_clipboard_text(text)?)
  }

  fn clear(&self) -> Result<(), Self::Error> {
    Ok(android::clear_clipboard()?)
  }
}
