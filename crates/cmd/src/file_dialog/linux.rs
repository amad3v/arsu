//! GTK's file chooser.

use std::{cell::Cell, path::PathBuf, sync::mpsc};

use gtk::{
  FileChooserAction, FileChooserNative, FileFilter, ResponseType,
  prelude::{FileChooserExt, NativeDialogExt},
};
use tauri::WebviewWindow;

use super::{FileDialog, FileType, Purpose};
use crate::error::AppError;

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
