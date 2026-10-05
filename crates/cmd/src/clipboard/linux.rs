//! The Linux clipboard, through `arboard` (X11 and Wayland).

use std::sync::{Mutex, MutexGuard, PoisonError};

use arboard::SetExtLinux;

use super::SystemClipboard;

/// What can go wrong reading, writing or clearing the system clipboard.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
  /// No clipboard could be opened for this process — captured once, at
  /// [`Clipboard::new`], and replayed by every operation rather than
  /// retried.
  #[error("the clipboard is not available: {0}")]
  Unavailable(String),
  /// [`Clipboard::close`] already released the clipboard (the app is
  /// exiting).
  #[error("the clipboard has already been released")]
  Closed,
  #[error(transparent)]
  Operation(#[from] arboard::Error),
}

/// The process's system clipboard, owned directly through `arboard`
/// rather than a Tauri plugin, so a copied code can be excluded from
/// clipboard-manager history (see the module docs) — something no
/// plugin exposed a way to do.
///
/// `arboard::Clipboard` must be dropped for its contents to survive the
/// process past that point (see its own docs), and Tauri does not drop
/// managed state on exit, so [`Self::close`] must be called once, by
/// hand, from a `RunEvent::Exit` handler.
pub struct Clipboard(Result<Mutex<Option<arboard::Clipboard>>, String>);

impl Clipboard {
  /// Opens the system clipboard. Never fails: if this environment has no
  /// clipboard support at all, that failure is captured here and
  /// returned by every later operation instead.
  #[must_use]
  pub fn new() -> Self {
    Self(
      arboard::Clipboard::new()
        .map(|clipboard| Mutex::new(Some(clipboard)))
        .map_err(|error| error.to_string()),
    )
  }

  /// Releases the clipboard, handing its contents to a running
  /// clipboard manager if there is one. Every later operation then
  /// fails with [`ClipboardError::Closed`] instead of reopening it.
  pub fn close(&self) {
    if let Ok(clipboard) = &self.0 {
      lock(clipboard).take();
    }
  }

  fn with<T>(
    &self,
    op: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
  ) -> Result<T, ClipboardError> {
    match &self.0 {
      Err(reason) => Err(ClipboardError::Unavailable(reason.clone())),
      Ok(clipboard) => match lock(clipboard).as_mut() {
        Some(clipboard) => op(clipboard).map_err(ClipboardError::Operation),
        None => Err(ClipboardError::Closed),
      },
    }
  }
}

impl Default for Clipboard {
  fn default() -> Self {
    Self::new()
  }
}

fn lock(mutex: &Mutex<Option<arboard::Clipboard>>) -> MutexGuard<'_, Option<arboard::Clipboard>> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SystemClipboard for Clipboard {
  type Error = ClipboardError;

  fn read_text(&self) -> Result<String, Self::Error> {
    self.with(arboard::Clipboard::get_text)
  }

  fn write_text(&self, text: &str) -> Result<(), Self::Error> {
    // `exclude_from_history` sets the KDE/GNOME "password manager hint"
    // (Klipper, GPaste) and Wayland's equivalent, so a code is never
    // written to persisted clipboard history: `clear_if_unchanged`'s
    // clear cannot otherwise be undone by the history restoring it.
    self.with(|clipboard| clipboard.set().exclude_from_history().text(text))
  }

  fn clear(&self) -> Result<(), Self::Error> {
    self.with(arboard::Clipboard::clear)
  }
}
