//! The Tauri command layer: the commands the frontend calls
//! ([`commands`]), the wire types they take and return ([`dto`]), the
//! error they fail with ([`error`]), and the state behind them
//! ([`AppState`]).
//!
//! This is the trust boundary between the `WebView` and the domain crates
//! (`crypto`, `vault-core`, `otp`, `qr`, `storage`, `interop`): every
//! input is validated here or below, and decrypted secrets stay on this
//! side except in the few places [`dto`] lists. The frontend mirrors this
//! surface in `src/api/` and `src/types/api.ts`.

pub mod about;
#[cfg(target_os = "android")]
pub mod android;
pub mod auto_lock;
pub mod biometric;
pub mod bluetooth;
pub mod clipboard;
pub mod clock;
pub mod commands;
pub mod device;
pub mod diagnostics;
pub mod dto;
pub mod error;
pub mod file_dialog;
pub mod files;
pub mod password;
#[cfg(target_os = "linux")]
pub mod pinch_zoom;
mod state;

pub use state::{AppState, app_paths};
