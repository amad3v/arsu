//! Native file dialogs: on Linux, GTK's file chooser, modal to the app's
//! window; on Android, the system's document picker, which hands the app
//! a `content://` URI rather than a path (see [`ChosenFile`]).
//!
//! Not `tauri-plugin-dialog` on Linux: there its `rfd` backend ignores
//! the parent window, so its dialogs float free of the app. Clicking the
//! app then took focus from the dialog, and closed the app's own dialog
//! behind it as a click outside. A chooser that is modal and transient
//! for the window keeps the app from taking input while it is open, and
//! stays above it. On Android the plugin is the only way to the picker,
//! which needs an activity result; it is driven from Rust, and the
//! `WebView` is granted none of its commands.

#[cfg(target_os = "linux")]
#[path = "file_dialog/linux.rs"]
mod backend;

#[cfg(target_os = "android")]
#[path = "file_dialog/android.rs"]
mod backend;

#[cfg(target_os = "android")]
pub use backend::Document;

/// A file the user chose in a dialog, to read or write: a path on Linux,
/// a picked document on Android. Either way, [`crate::files::UserFile`]
/// reads and writes it.
#[cfg(target_os = "linux")]
pub type ChosenFile = std::path::PathBuf;
#[cfg(target_os = "android")]
pub type ChosenFile = Document;

/// One entry of the dialog's file type list. Its name lists the patterns,
/// as in "Aegis backup (*.json)", so the user sees what it matches.
#[derive(Debug, Clone, Copy)]
pub struct FileType {
  pub name: &'static str,
  /// Glob patterns on the file name, e.g. `*.json`.
  pub patterns: &'static [&'static str],
  /// MIME types, for file types that GTK recognises by content.
  pub mime_types: &'static [&'static str],
}

/// What the dialog is for.
#[derive(Debug, Clone, Copy)]
pub enum Purpose {
  Open,
  /// Choose where to save, starting from `file_name`; replacing an
  /// existing file is confirmed.
  Save {
    file_name: &'static str,
  },
}

/// A file dialog to show.
#[derive(Debug, Clone, Copy)]
pub struct FileDialog {
  pub title: &'static str,
  pub purpose: Purpose,
  /// The file types offered, the first one selected.
  pub file_types: &'static [FileType],
}
