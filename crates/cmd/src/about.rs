//! The About dialog's backend: what it shows, the text "Copy details"
//! puts on the clipboard, and opening the project's pages.

use std::{path::Path, sync::mpsc};

use tauri::{AppHandle, WebviewWindow};

use crate::{
  dto::{AboutInfo, AppLink},
  error::AppError,
  state::AppState,
};

/// The project's home page.
const WEBSITE_URL: &str = "https://github.com/amad3v/arsu";
/// Where to report a bug.
const ISSUES_URL: &str = "https://github.com/amad3v/arsu/issues";

impl AppLink {
  #[must_use]
  pub const fn url(self) -> &'static str {
    match self {
      Self::Website => WEBSITE_URL,
      Self::Issues => ISSUES_URL,
    }
  }
}

impl AboutInfo {
  /// The app's name and version (from its package info), the Tauri and
  /// `WebKitGTK` versions it runs on, and its files' paths.
  #[must_use]
  pub fn new(app: &AppHandle, state: &AppState) -> Self {
    let package = app.package_info();
    Self {
      name: package.name.clone(),
      version: package.version.to_string(),
      tauri_version: tauri::VERSION.to_owned(),
      webview_version: tauri::webview_version().ok(),
      vault_path: display(state.paths().vault()),
      settings_path: display(state.paths().settings()),
    }
  }

  /// The details as plain text, one per line, for a bug report.
  #[must_use]
  pub fn details_text(&self) -> String {
    let webview = self.webview_version.as_deref().unwrap_or("unknown");
    format!(
      "{} {}\nTauri {}\nWebKitGTK {webview}\nVault: {}\nSettings: {}\n",
      self.name, self.version, self.tauri_version, self.vault_path, self.settings_path,
    )
  }
}

fn display(path: &Path) -> String {
  path.to_string_lossy().into_owned()
}

/// Opens `link` in the user's browser, through GTK (and the desktop
/// portal where there is one), over `window`.
///
/// Blocks the calling thread, so it must not be the main thread, which
/// opens it.
///
/// # Errors
///
/// Returns [`AppError::BackgroundTask`] if the main thread can't be
/// reached, and [`AppError::OpenLink`] if no application opens the link.
pub fn open_link(window: &WebviewWindow, link: AppLink) -> Result<(), AppError> {
  let (answer, answered) = mpsc::sync_channel(1);
  let main_window = window.clone();
  window
    .run_on_main_thread(move || {
      let opened = main_window
        .gtk_window()
        .map_err(AppError::BackgroundTask)
        .and_then(|parent| {
          // 0 is GDK_CURRENT_TIME: there is no event to take a time from.
          gtk::show_uri_on_window(Some(&parent), link.url(), 0).map_err(AppError::OpenLink)
        });
      // The receiver is dropped only if the command was; nobody to tell.
      let _ = answer.send(opened);
    })
    .map_err(AppError::BackgroundTask)?;
  // A dropped sender: the closure never ran (the app is shutting down).
  answered.recv().unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn links_open_only_the_projects_pages() {
    assert_eq!(AppLink::Website.url(), "https://github.com/amad3v/arsu");
    assert_eq!(
      AppLink::Issues.url(),
      "https://github.com/amad3v/arsu/issues"
    );
  }

  #[test]
  fn details_are_one_per_line() {
    let info = AboutInfo {
      name: "Arsu".to_owned(),
      version: "1.0.0".to_owned(),
      tauri_version: "2.11.6".to_owned(),
      webview_version: None,
      vault_path: "/home/u/.local/share/arsu/vault".to_owned(),
      settings_path: "/home/u/.config/arsu/settings.json".to_owned(),
    };
    assert_eq!(
      info.details_text(),
      "Arsu 1.0.0\nTauri 2.11.6\nWebKitGTK unknown\n\
       Vault: /home/u/.local/share/arsu/vault\n\
       Settings: /home/u/.config/arsu/settings.json\n"
    );
  }
}
