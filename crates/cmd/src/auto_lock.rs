//! Locking the vault once the user has been idle for the configured time,
//! and on Android also as soon as the phone is locked or its screen goes
//! off, whatever that time.
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
/// can be — including after waking from suspend. On Android, also how
/// late the lock can follow the phone's own: short, so that turning the
/// screen off and straight back on still finds the vault locked.
#[cfg(not(target_os = "android"))]
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(target_os = "android")]
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

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
        let state = app.state::<AppState>();
        if state.lock_if_idle(BootInstant::now()) || (phone_locked() && state.lock_if_unlocked()) {
          // The vault is locked either way; if no window is left to
          // tell, there is nothing more to do.
          let _ = app.emit(VAULT_LOCKED_EVENT, ());
        }
      }
    })?;
  Ok(())
}

/// Whether the phone is locked or its screen is off. A failed check
/// counts as not locked: the idle lock still applies.
#[cfg(target_os = "android")]
fn phone_locked() -> bool {
  crate::android::phone_locked().unwrap_or(false)
}

/// The desktop has no such signal of its own here.
#[cfg(not(target_os = "android"))]
const fn phone_locked() -> bool {
  false
}
