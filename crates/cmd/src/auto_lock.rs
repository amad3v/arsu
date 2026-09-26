//! Locking the vault once the user has been idle for the configured time.
//!
//! The backend owns this rather than the `WebView`: a stalled, reloaded or
//! compromised frontend cannot keep the key in memory. User-initiated
//! commands and `record_activity` postpone the lock; the entry list's
//! automatic code refreshes do not. Idle time is measured on
//! `CLOCK_BOOTTIME` ([`BootInstant`]), so time spent suspended counts.

use std::{io, thread, time::Duration};

use tauri::{AppHandle, Emitter, Manager};

use crate::{clock::BootInstant, state::AppState};

/// Emitted, with no payload, whenever the backend locks the vault on
/// its own; the frontend then shows the unlock screen.
pub const VAULT_LOCKED_EVENT: &str = "vault-locked";

/// How often the idle time is checked, and so roughly how late a lock
/// can be — including after waking from suspend.
const CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Starts the thread that locks the vault when it has been idle for the
/// auto-lock time, emitting [`VAULT_LOCKED_EVENT`] each time it does.
/// `app` must already manage the [`AppState`].
///
/// # Errors
///
/// Returns an error if the thread cannot be started.
pub fn spawn(app: AppHandle) -> io::Result<()> {
  thread::Builder::new()
    .name("auto-lock".to_owned())
    .spawn(move || {
      loop {
        thread::sleep(CHECK_INTERVAL);
        if app.state::<AppState>().lock_if_idle(BootInstant::now()) {
          // The vault is locked either way; if no window is left to
          // tell, there is nothing more to do.
          let _ = app.emit(VAULT_LOCKED_EVENT, ());
        }
      }
    })?;
  Ok(())
}
