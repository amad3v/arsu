//! Copying codes to the clipboard, and clearing them again.
//!
//! The copy happens in Rust: the frontend holds no clipboard permission
//! at all. Each copy is remembered only by the SHA-256 digest of the
//! code, and after the configured delay the clipboard is cleared — but
//! only if it still holds that very code, and only if no later copy
//! replaced it: whatever the user copied since is never touched. On
//! Linux, a copied code is also excluded from clipboard-manager history
//! (Klipper, `GPaste`, cliphist, …), so the clear cannot be undone by the
//! history restoring it.

use std::sync::{Mutex, MutexGuard, PoisonError};

use arboard::SetExtLinux;
use sha2::{Digest, Sha256};
use tauri::{Manager, Runtime};
use zeroize::Zeroizing;

use crate::error::AppError;

/// A code the frontend asks to copy: 6 to 8 ASCII digits, the only
/// thing this app ever puts on the clipboard.
#[derive(Debug)]
pub struct OtpCode(String);

impl OtpCode {
  /// # Errors
  ///
  /// Returns [`AppError::InvalidCode`] unless `code` is 6 to 8 ASCII
  /// digits.
  pub fn parse(code: String) -> Result<Self, AppError> {
    if (6..=8).contains(&code.len()) && code.bytes().all(|byte| byte.is_ascii_digit()) {
      Ok(Self(code))
    } else {
      Err(AppError::InvalidCode)
    }
  }

  #[must_use]
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

type Digest256 = [u8; 32];

fn digest(text: &str) -> Digest256 {
  Sha256::digest(text.as_bytes()).into()
}

/// One copy, as remembered until its clearing is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyTicket {
  serial: u64,
  digest: Digest256,
}

#[derive(Debug, Default)]
struct CopyLog {
  copies: u64,
  pending: Option<CopyTicket>,
}

/// The most recent copy whose clearing is still pending.
#[derive(Debug, Default)]
pub struct CopiedCodes(Mutex<CopyLog>);

impl CopiedCodes {
  /// Remembers that `code` was just copied, superseding any earlier
  /// copy, and returns the ticket to clear it with.
  pub fn record(&self, code: &OtpCode) -> CopyTicket {
    let mut log = self.log();
    log.copies += 1;
    let ticket = CopyTicket {
      serial: log.copies,
      digest: digest(code.as_str()),
    };
    log.pending = Some(ticket);
    ticket
  }

  /// The ticket of the copy still waiting to be cleared, if any.
  #[must_use]
  pub fn pending(&self) -> Option<CopyTicket> {
    self.log().pending
  }

  /// Whether `ticket` is still the latest copy. If it is, it is
  /// forgotten: its clearing is happening now.
  fn take_if_latest(&self, ticket: CopyTicket) -> bool {
    let mut log = self.log();
    let latest = log.pending == Some(ticket);
    if latest {
      log.pending = None;
    }
    latest
  }

  fn log(&self) -> MutexGuard<'_, CopyLog> {
    // Every critical section leaves the log consistent, so a poisoned
    // lock (a panic elsewhere while holding it) is safe to keep using.
    self.0.lock().unwrap_or_else(PoisonError::into_inner)
  }
}

/// The clipboard operations copying and clearing a code needs.
pub trait SystemClipboard {
  type Error;

  /// The clipboard's text, or an error if it holds none.
  ///
  /// # Errors
  ///
  /// Returns an error if the clipboard holds no text or cannot be read.
  fn read_text(&self) -> Result<String, Self::Error>;

  /// Places `text` on the clipboard.
  ///
  /// # Errors
  ///
  /// Returns an error if the clipboard cannot be written to.
  fn write_text(&self, text: &str) -> Result<(), Self::Error>;

  /// # Errors
  ///
  /// Returns an error if the clipboard cannot be cleared.
  fn clear(&self) -> Result<(), Self::Error>;
}

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

/// Mirrors `tauri_plugin_clipboard_manager::ClipboardExt`, the plugin
/// this replaces: `app.clipboard()` reaches the [`Clipboard`] Tauri
/// manages.
pub trait ClipboardExt<R: Runtime> {
  fn clipboard(&self) -> &Clipboard;
}

impl<R: Runtime, T: Manager<R>> ClipboardExt<R> for T {
  fn clipboard(&self) -> &Clipboard {
    self.state::<Clipboard>().inner()
  }
}

/// The clipboard plugin's own exit duty, now done by hand: clear a code
/// still waiting to be cleared, then release the clipboard so its last
/// contents survive the process. Call this once, from a
/// `RunEvent::Exit` handler, before the process exits.
pub fn shut_down(clipboard: &Clipboard, copies: &CopiedCodes) {
  if let Some(ticket) = copies.pending() {
    // Best effort: nothing is waiting for the result, and there is
    // nothing more to do before exiting either way.
    let _ = clear_if_unchanged(clipboard, copies, ticket);
  }
  clipboard.close();
}

/// Clears the clipboard if `ticket` is still the latest copy and the
/// clipboard still holds exactly its code. Returns whether it cleared.
///
/// A clipboard that cannot be read, or holds no text, holds something
/// else than the code, so it is left alone. Reading must not happen on
/// the main thread (`arboard`'s X11 backend can deadlock there).
///
/// # Errors
///
/// Returns the clipboard's error if clearing it fails.
pub fn clear_if_unchanged<C: SystemClipboard>(
  clipboard: &C,
  copies: &CopiedCodes,
  ticket: CopyTicket,
) -> Result<bool, C::Error> {
  if !copies.take_if_latest(ticket) {
    return Ok(false);
  }
  let Ok(text) = clipboard.read_text().map(Zeroizing::new) else {
    return Ok(false);
  };
  if digest(&text) != ticket.digest {
    return Ok(false);
  }
  clipboard.clear()?;
  Ok(true)
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use super::*;

  /// An in-memory clipboard; `None` means it holds no text.
  struct FakeClipboard(RefCell<Option<String>>);

  impl FakeClipboard {
    fn holding(text: &str) -> Self {
      Self(RefCell::new(Some(text.to_owned())))
    }

    fn text(&self) -> Option<String> {
      self.0.borrow().clone()
    }
  }

  impl SystemClipboard for FakeClipboard {
    type Error = ();

    fn read_text(&self) -> Result<String, ()> {
      self.0.borrow().clone().ok_or(())
    }

    fn write_text(&self, text: &str) -> Result<(), ()> {
      self.0.replace(Some(text.to_owned()));
      Ok(())
    }

    fn clear(&self) -> Result<(), ()> {
      self.0.replace(None);
      Ok(())
    }
  }

  fn code(digits: &str) -> OtpCode {
    OtpCode::parse(digits.to_owned()).unwrap()
  }

  #[test]
  fn only_6_to_8_ascii_digits_can_be_copied() {
    for valid in ["123456", "1234567", "12345678"] {
      assert_eq!(OtpCode::parse(valid.to_owned()).unwrap().as_str(), valid);
    }
    for invalid in [
      "",
      "12345",
      "123456789",
      "12a456",
      "123 456",
      "١٢٣٤٥٦",
      "password",
    ] {
      assert!(
        matches!(
          OtpCode::parse(invalid.to_owned()),
          Err(AppError::InvalidCode)
        ),
        "{invalid:?}"
      );
    }
  }

  #[test]
  fn clears_a_code_still_on_the_clipboard() {
    let copies = CopiedCodes::default();
    let clipboard = FakeClipboard::holding("123456");
    let ticket = copies.record(&code("123456"));

    assert_eq!(clear_if_unchanged(&clipboard, &copies, ticket), Ok(true));
    assert_eq!(clipboard.text(), None);
  }

  #[test]
  fn leaves_whatever_the_user_copied_since() {
    let copies = CopiedCodes::default();
    let clipboard = FakeClipboard::holding("my other text");
    let ticket = copies.record(&code("123456"));

    assert_eq!(clear_if_unchanged(&clipboard, &copies, ticket), Ok(false));
    assert_eq!(clipboard.text().as_deref(), Some("my other text"));
  }

  #[test]
  fn leaves_a_clipboard_without_text() {
    let copies = CopiedCodes::default();
    let clipboard = FakeClipboard(RefCell::new(None));
    let ticket = copies.record(&code("123456"));

    assert_eq!(clear_if_unchanged(&clipboard, &copies, ticket), Ok(false));
  }

  #[test]
  fn a_later_copy_of_the_same_code_restarts_the_delay() {
    let copies = CopiedCodes::default();
    let clipboard = FakeClipboard::holding("123456");
    let first = copies.record(&code("123456"));
    let second = copies.record(&code("123456"));

    assert_eq!(clear_if_unchanged(&clipboard, &copies, first), Ok(false));
    assert_eq!(clipboard.text().as_deref(), Some("123456"));
    assert_eq!(clear_if_unchanged(&clipboard, &copies, second), Ok(true));
  }

  #[test]
  fn a_ticket_clears_at_most_once() {
    let copies = CopiedCodes::default();
    let ticket = copies.record(&code("123456"));
    assert_eq!(
      clear_if_unchanged(&FakeClipboard::holding("123456"), &copies, ticket),
      Ok(true)
    );

    let clipboard = FakeClipboard::holding("123456");
    assert_eq!(clear_if_unchanged(&clipboard, &copies, ticket), Ok(false));
    assert_eq!(clipboard.text().as_deref(), Some("123456"));
  }

  #[test]
  fn pending_reflects_the_copy_still_waiting_to_be_cleared() {
    let copies = CopiedCodes::default();
    assert_eq!(copies.pending(), None);

    let ticket = copies.record(&code("123456"));
    assert_eq!(copies.pending(), Some(ticket));

    let clipboard = FakeClipboard::holding("123456");
    clear_if_unchanged(&clipboard, &copies, ticket).unwrap();
    assert_eq!(copies.pending(), None, "cleared: no longer pending");
  }
}
