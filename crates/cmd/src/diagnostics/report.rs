//! The diagnostic report: one zip
//! file the user saves and attaches to a bug report, after seeing what it
//! holds.
//!
//! * `environment.json`: what the app and the device are, the state the
//!   app was in (never what the vault holds), and the log files below;
//! * `logs/arsu.N.jsonl`, oldest first: the log files as they are, masked
//!   as they were written.

use std::{
  io::{Cursor, Write},
  sync::Arc,
};

use serde::Serialize;
use storage::Theme;
use time::OffsetDateTime;
use zip::{CompressionMethod, DateTime, ZipWriter, result::ZipError, write::SimpleFileOptions};

use super::{environment::Environment, event::timestamp, files::LogFile};
use crate::{
  dto::{DiagnosticReportPreview, Settings},
  error::AppError,
};

/// The report's format, for whoever reads it.
const FORMAT: &str = "arsu-diagnostic-report/1";

/// Records the preview shows, at most: the latest.
pub const PREVIEW_RECORDS: usize = 500;

/// Where the app was when the report was made: nothing the vault holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppSummary {
  pub vault: VaultState,
  /// The devices the vault trusts, this one included, when unlocked.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub trusted_devices: Option<usize>,
  /// Whether biometric unlock is on, where it can be told.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub biometric_unlock: Option<bool>,
  /// The settings in effect, where they can be read.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub settings: Option<SettingsSummary>,
}

/// Whether there is a vault, and whether it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
  /// None created yet.
  Missing,
  Locked,
  Unlocked,
  /// It cannot be told (another instance holds the vault, or its
  /// directory cannot be read).
  Unknown,
}

/// The settings, as the report names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SettingsSummary {
  pub theme: Theme,
  pub auto_lock_minutes: u32,
  pub clipboard_clear_seconds: u32,
}

impl From<Settings> for SettingsSummary {
  fn from(settings: Settings) -> Self {
    Self {
      theme: settings.theme,
      auto_lock_minutes: settings.auto_lock_minutes,
      clipboard_clear_seconds: settings.clipboard_clear_seconds,
    }
  }
}

/// A report, made and kept until the user saves it.
#[derive(Debug, Clone)]
pub struct Report {
  /// The name the save dialog suggests.
  pub file_name: String,
  /// The zip file.
  pub bytes: Arc<[u8]>,
  environment: String,
  records: usize,
  latest: Vec<String>,
}

/// `environment.json`.
#[derive(Serialize)]
struct Manifest<'a> {
  format: &'static str,
  created: String,
  /// The app run that made the report.
  session: &'a str,
  environment: &'a Environment,
  app: &'a AppSummary,
  logs: Vec<LogEntry>,
}

#[derive(Serialize)]
struct LogEntry {
  file: String,
  records: usize,
  bytes: usize,
}

/// The report of `logs` (oldest first), on the device `environment`
/// describes, the app as `app` says, made by the app run `session` at
/// `now`.
///
/// # Errors
///
/// [`AppError::DiagnosticReport`] if the zip file cannot be put together.
pub fn bundle(
  logs: &[LogFile],
  environment: &Environment,
  app: &AppSummary,
  session: &str,
  now: OffsetDateTime,
) -> Result<Report, AppError> {
  let records = |log: &LogFile| log.text().lines().filter(|line| !line.is_empty()).count();
  let manifest = Manifest {
    format: FORMAT,
    created: timestamp(now),
    session,
    environment,
    app,
    logs: logs
      .iter()
      .map(|log| LogEntry {
        file: format!("logs/{}", log.name),
        records: records(log),
        bytes: log.bytes.len(),
      })
      .collect(),
  };
  // A struct of strings, numbers and options always serializes.
  let environment_json = serde_json::to_string_pretty(&manifest).unwrap_or_default();

  let options = SimpleFileOptions::default()
    .compression_method(CompressionMethod::Deflated)
    .last_modified_time(zip_time(now));
  let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
  zip.start_file("environment.json", options)?;
  zip
    .write_all(environment_json.as_bytes())
    .map_err(ZipError::Io)?;
  for (log, entry) in logs.iter().zip(&manifest.logs) {
    zip.start_file(entry.file.as_str(), options)?;
    zip.write_all(&log.bytes).map_err(ZipError::Io)?;
  }
  let bytes = zip.finish()?.into_inner();

  let mut latest: Vec<String> = logs
    .iter()
    .rev()
    .flat_map(|log| {
      log
        .text()
        .lines()
        .rev()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>()
    })
    .take(PREVIEW_RECORDS)
    .collect();
  latest.reverse();

  Ok(Report {
    file_name: format!(
      "arsu-diagnostics-{:04}{:02}{:02}-{:02}{:02}{:02}.zip",
      now.year(),
      u8::from(now.month()),
      now.day(),
      now.hour(),
      now.minute(),
      now.second()
    ),
    bytes: Arc::from(bytes),
    environment: environment_json,
    records: manifest.logs.iter().map(|entry| entry.records).sum(),
    latest,
  })
}

impl Report {
  /// What the user sees of the report before saving it.
  #[must_use]
  pub fn preview(&self) -> DiagnosticReportPreview {
    DiagnosticReportPreview {
      file_name: self.file_name.clone(),
      bytes: self.bytes.len(),
      environment: self.environment.clone(),
      records: self.records,
      latest: self.latest.join("\n"),
      latest_records: self.latest.len(),
    }
  }
}

/// `at` as a zip file's time: UTC, to the second (two, as zip keeps it).
fn zip_time(at: OffsetDateTime) -> DateTime {
  u16::try_from(at.year())
    .ok()
    .and_then(|year| {
      DateTime::from_date_and_time(
        year,
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
      )
      .ok()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use std::{fmt::Write as _, io::Read};

  use serde_json::Value as Json;
  use time::macros::datetime;
  use zip::ZipArchive;

  use super::*;

  const NOW: OffsetDateTime = datetime!(2026-10-11 14:03:22.114 UTC);

  fn app() -> AppSummary {
    AppSummary {
      vault: VaultState::Unlocked,
      trusted_devices: Some(3),
      biometric_unlock: Some(false),
      settings: Some(SettingsSummary {
        theme: Theme::Dark,
        auto_lock_minutes: 5,
        clipboard_clear_seconds: 30,
      }),
    }
  }

  fn logs() -> Vec<LogFile> {
    vec![
      LogFile {
        name: "arsu.1.jsonl".to_owned(),
        bytes: b"{\"type\":\"env\"}\n{\"seq\":1}\n".to_vec(),
      },
      LogFile {
        name: "arsu.jsonl".to_owned(),
        bytes: b"{\"type\":\"env\"}\n{\"seq\":2}\n{\"seq\":3}\n".to_vec(),
      },
    ]
  }

  fn environment() -> Environment {
    Environment::detect("Arsu", "1.2.0", "abc123", "WebKitGTK 2.46".to_owned())
  }

  fn read(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> String {
    let mut text = String::new();
    zip
      .by_name(name)
      .unwrap()
      .read_to_string(&mut text)
      .unwrap();
    text
  }

  #[test]
  fn a_report_holds_the_environment_then_the_logs_oldest_first() {
    let report = bundle(&logs(), &environment(), &app(), "S", NOW).unwrap();

    assert_eq!(report.file_name, "arsu-diagnostics-20261011-140322.zip");
    let mut zip = ZipArchive::new(Cursor::new(&report.bytes[..])).unwrap();
    assert_eq!(
      zip.file_names().collect::<Vec<_>>().len(),
      3,
      "environment.json and two log files"
    );
    assert_eq!(
      read(&mut zip, "logs/arsu.1.jsonl"),
      "{\"type\":\"env\"}\n{\"seq\":1}\n"
    );
    let manifest: Json = serde_json::from_str(&read(&mut zip, "environment.json")).unwrap();
    assert_eq!(manifest["format"], FORMAT);
    assert_eq!(manifest["created"], "2026-10-11T14:03:22.114Z");
    assert_eq!(manifest["session"], "S");
    assert_eq!(manifest["environment"]["app"]["version"], "1.2.0");
    assert_eq!(manifest["app"]["vault"], "unlocked");
    assert_eq!(manifest["app"]["trusted_devices"], 3);
    assert_eq!(manifest["app"]["settings"]["theme"], "dark");
    assert_eq!(manifest["logs"][0]["file"], "logs/arsu.1.jsonl");
    assert_eq!(manifest["logs"][0]["records"], 2);
    assert_eq!(manifest["logs"][1]["file"], "logs/arsu.jsonl");
  }

  #[test]
  fn the_preview_shows_exactly_what_the_report_holds() {
    let report = bundle(&logs(), &environment(), &app(), "S", NOW).unwrap();

    let preview = report.preview();

    let mut zip = ZipArchive::new(Cursor::new(&report.bytes[..])).unwrap();
    assert_eq!(preview.environment, read(&mut zip, "environment.json"));
    assert_eq!(preview.records, 5);
    assert_eq!(preview.latest_records, 5);
    assert_eq!(
      preview.latest,
      "{\"type\":\"env\"}\n{\"seq\":1}\n{\"type\":\"env\"}\n{\"seq\":2}\n{\"seq\":3}"
    );
    assert_eq!(preview.bytes, report.bytes.len());
  }

  #[test]
  fn the_preview_shows_only_the_latest_records() {
    let many = (0..PREVIEW_RECORDS + 20).fold(String::new(), |mut many, seq| {
      let _ = writeln!(many, "{{\"seq\":{seq}}}");
      many
    });
    let logs = [LogFile {
      name: "arsu.jsonl".to_owned(),
      bytes: many.into_bytes(),
    }];

    let preview = bundle(&logs, &environment(), &app(), "S", NOW)
      .unwrap()
      .preview();

    assert_eq!(preview.records, PREVIEW_RECORDS + 20);
    assert_eq!(preview.latest_records, PREVIEW_RECORDS);
    assert!(preview.latest.starts_with("{\"seq\":20}\n"));
  }
}
