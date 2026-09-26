//! Native file dialogs: GTK's file chooser, modal to the app's window.
//!
//! Not `tauri-plugin-dialog`: on Linux its `rfd` backend ignores the
//! parent window, so its dialogs float free of the app. Clicking the app
//! then took focus from the dialog, and closed the app's own dialog
//! behind it as a click outside. A chooser that is modal and transient
//! for the window keeps the app from taking input while it is open, and
//! stays above it.

use std::{cell::Cell, path::PathBuf, sync::mpsc};

use gtk::{
  FileChooserAction, FileChooserNative, FileFilter, ResponseType,
  prelude::{FileChooserExt, NativeDialogExt},
};
use tauri::WebviewWindow;

use crate::error::AppError;

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

type Answer = Result<Option<PathBuf>, AppError>;

impl FileDialog {
  /// Shows the dialog over `window` and waits for the user's answer: the
  /// chosen path, or `None` if they cancel (or the window goes away).
  ///
  /// Blocks the calling thread, so it must not be the main thread, which
  /// runs the dialog.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::FileDialog`] if the dialog cannot be shown, and
  /// [`AppError::InvalidFilePath`] if the choice is not a local file.
  pub fn show(self, window: &WebviewWindow) -> Answer {
    let (answer, answered) = mpsc::sync_channel::<Answer>(1);
    let dialog_window = window.clone();
    window
      .run_on_main_thread(move || {
        if let Err(error) = self.open_over(&dialog_window, answer.clone()) {
          // The receiver outlives the dialog unless the command was
          // dropped, in which case there is no one left to tell.
          let _ = answer.send(Err(error));
        }
      })
      .map_err(AppError::FileDialog)?;
    // A dropped sender means the dialog was destroyed unanswered.
    answered.recv().unwrap_or(Ok(None))
  }

  /// Builds and shows the chooser; its response sends the answer. Runs on
  /// the main thread.
  fn open_over(
    self,
    window: &WebviewWindow,
    answer: mpsc::SyncSender<Answer>,
  ) -> Result<(), AppError> {
    let parent = window.gtk_window().map_err(AppError::FileDialog)?;
    let (action, accept) = match self.purpose {
      Purpose::Open => (FileChooserAction::Open, "_Open"),
      Purpose::Save { .. } => (FileChooserAction::Save, "_Save"),
    };
    let chooser = FileChooserNative::new(
      Some(self.title),
      Some(&parent),
      action,
      Some(accept),
      Some("_Cancel"),
    );
    chooser.set_modal(true);
    chooser.set_local_only(true);
    if let Purpose::Save { file_name } = self.purpose {
      chooser.set_do_overwrite_confirmation(true);
      chooser.set_current_name(file_name);
    }
    for file_type in self.file_types {
      chooser.add_filter(file_type.filter());
    }

    // GTK does not keep a native dialog alive while it is shown: its
    // response handler holds it until it answers.
    let keep_alive = Cell::new(Some(chooser.clone()));
    chooser.connect_response(move |chooser, response| {
      let choice = match response {
        ResponseType::Accept => chooser
          .filename()
          .ok_or(AppError::InvalidFilePath)
          .map(Some),
        _ => Ok(None),
      };
      let _ = answer.send(choice);
      chooser.destroy();
      keep_alive.take();
    });
    chooser.show();
    Ok(())
  }
}

impl FileType {
  fn filter(&self) -> FileFilter {
    let filter = FileFilter::new();
    filter.set_name(Some(self.name));
    for pattern in self.patterns {
      filter.add_pattern(pattern);
    }
    for mime_type in self.mime_types {
      filter.add_mime_type(mime_type);
    }
    filter
  }
}
