//! The system's document picker, through `tauri-plugin-dialog`.

use tauri::{Url, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FilePath, PickerMode};

use super::{FileDialog, Purpose};
use crate::{android, error::AppError};

/// A document the user picked: its `content://` URI, which the app was
/// granted access to, and the name its provider shows it under.
#[derive(Debug, Clone)]
pub struct Document {
  pub(crate) uri: String,
  /// `None` if its provider doesn't give one.
  pub(crate) name: Option<String>,
}

impl Document {
  fn from_picked(picked: FilePath) -> Result<Self, AppError> {
    let uri = match picked {
      FilePath::Url(url) => url,
      FilePath::Path(path) => Url::from_file_path(&path).map_err(|()| AppError::InvalidFilePath)?,
    };
    let uri = uri.to_string();
    // The name is only shown, and decides an import's format: a provider
    // that cannot give one leaves the import refused, not the app broken.
    let name = android::document_name(&uri).ok().flatten();
    Ok(Self { uri, name })
  }
}

type Answer = Result<Option<Document>, AppError>;

impl FileDialog {
  /// Shows the picker and waits for the user's answer: the chosen
  /// document, or `None` if they cancel.
  ///
  /// Blocks the calling thread, so it must not be the main thread, which
  /// the picker's result comes back through.
  ///
  /// The file types only narrow the picker to images, when every type
  /// offered is one: the picker filters on MIME types, and backups such
  /// as `.2fas` files have none it knows, so it would hide them. What is
  /// picked is checked afterwards instead, as on Linux.
  ///
  /// # Errors
  ///
  /// Returns [`AppError::InvalidFilePath`] if the picker returns a
  /// location that is neither a URI nor an absolute path.
  pub fn show(self, window: &WebviewWindow) -> Answer {
    let picker = window.dialog().file().set_title(self.title);
    let choice = match self.purpose {
      Purpose::Open => {
        let only_images = !self.file_types.is_empty()
          && self.file_types.iter().all(|file_type| {
            !file_type.mime_types.is_empty()
              && file_type
                .mime_types
                .iter()
                .all(|mime_type| mime_type.starts_with("image/"))
          });
        let picker = if only_images {
          picker.set_picker_mode(PickerMode::Image)
        } else {
          picker.set_picker_mode(PickerMode::Document)
        };
        picker.blocking_pick_file()
      }
      Purpose::Save { file_name } => {
        // The first type's extensions set the new document's MIME type.
        let extensions: Vec<&str> = self
          .file_types
          .first()
          .map(|file_type| {
            file_type
              .patterns
              .iter()
              .filter_map(|pattern| pattern.strip_prefix("*."))
              .collect()
          })
          .unwrap_or_default();
        picker
          .set_file_name(file_name)
          .add_filter(file_name, &extensions)
          .blocking_save_file()
      }
    };
    choice.map(Document::from_picked).transpose()
  }
}
