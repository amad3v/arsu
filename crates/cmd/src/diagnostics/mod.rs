//! The diagnostic log: what went wrong, on this device, for a bug report.
//!
//! * The log is JSON Lines (`event`) in the app's own log directory,
//!   which no other app can read, rotated at [`MAX_FILE_BYTES`] with
//!   [`KEPT_FILES`] files kept (`files`); each file and each app run
//!   starts with what the app and the device are ([`Environment`]).
//! * Only warnings, errors and critical failures are written, each with
//!   the routine steps that led to it, which are otherwise kept in memory
//!   only (`logger`).
//! * Every failure the interface reports goes through [`record!`], which
//!   gives it the severity of its kind
//!   ([`AppErrorKind::severity`](crate::error::AppErrorKind::severity)); a
//!   panic, through [`log_panics`]; the interface's own errors, through
//!   [`record_ui_error`].
//! * Nothing from the vault: no account, secret, code, invitation or
//!   device name is ever logged, and every text is masked ([`redact`]) as
//!   its record is made, before it reaches a file.
//! * It never leaves the device unless the user saves a report
//!   ([`Diagnostics::prepare_report`], a [`Report`]), which they see first.

mod environment;
mod event;
mod files;
mod logger;
mod redact;
mod report;

use std::{
  backtrace::Backtrace,
  io, panic,
  path::Path,
  sync::{Arc, Mutex, PoisonError},
};

use log::{Log, kv::Value};
use time::OffsetDateTime;

pub use self::{
  environment::Environment,
  logger::Filter,
  redact::{MaskedAddress, redact},
  report::{AppSummary, Report, SettingsSummary, VaultState},
};
use self::{files::LogFiles, logger::DiagnosticLogger};
use crate::{
  dto::{DiagnosticReportPreview, UiError},
  error::AppError,
};

/// How bad a failure is. Failures that are none of these (a
/// wrong password, a mistyped code) are expected and not logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
  /// Something did not work, for a reason outside Arsu: Bluetooth off, a
  /// file of another app's newer format, a device out of reach.
  Warning,
  /// Arsu failed at what it was asked: a file not written, a sync broken
  /// off by a protocol error.
  Error,
  /// The app or the vault is at risk: a crash, a vault that cannot be
  /// read or saved, no randomness.
  Critical,
}

/// Bytes a log file holds before it is rotated.
pub const MAX_FILE_BYTES: u64 = 1_000_000;

/// Log files kept, the current one included.
pub const KEPT_FILES: usize = 5;

/// The log target of a panic ([`log_panics`]): where it happened is its
/// source file and line.
pub const PANIC_TARGET: &str = "panic";

/// The log target of the interface's own errors ([`record_ui_error`]).
pub const UI_TARGET: &str = "ui";

/// The diagnostic log of this app run, and the report last prepared from
/// it: managed by the app shell once [`Diagnostics::start`] succeeds.
pub struct Diagnostics {
  logger: &'static DiagnosticLogger,
  environment: Environment,
  session: Arc<str>,
  pending: Mutex<Option<Arc<Report>>>,
}

/// Why the diagnostic log could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
  #[error("could not open the log files")]
  Files(#[source] io::Error),
  #[error("another logger is already installed")]
  Logger(#[source] log::SetLoggerError),
}

impl Diagnostics {
  /// Starts the diagnostic log of a new app run in `dir`, on the device
  /// `environment` describes, hearing what `filter` lets through: the
  /// process's logger from now on.
  ///
  /// # Errors
  ///
  /// [`StartError::Files`] if the log files cannot be opened or written;
  /// [`StartError::Logger`] if a logger is already installed.
  pub fn start(dir: &Path, environment: Environment, filter: Filter) -> Result<Self, StartError> {
    let session: Arc<str> = Arc::from(uuid::Uuid::now_v7().to_string());
    let header = logger::header(&environment, &session, OffsetDateTime::now_utc());
    let files =
      LogFiles::open(dir, header, MAX_FILE_BYTES, KEPT_FILES).map_err(StartError::Files)?;
    // The process's logger lives as long as the process: `log` holds a
    // `&'static`, and so do these diagnostics, to read the files under
    // the logger's own lock.
    let logger: &'static DiagnosticLogger = Box::leak(Box::new(DiagnosticLogger::new(
      filter,
      Arc::clone(&session),
      files,
    )));
    log::set_logger(logger).map_err(StartError::Logger)?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(Self {
      logger,
      environment,
      session,
      pending: Mutex::new(None),
    })
  }

  /// The app run's id, in every record of it.
  #[must_use]
  pub fn session(&self) -> &str {
    &self.session
  }

  /// Prepares a report of the log as it is now and of `app`, made at
  /// `now`, and keeps it for [`Self::prepared_report`]; returns what the user
  /// sees of it before saving it.
  ///
  /// # Errors
  ///
  /// [`AppError::FileRead`] if a log file cannot be read;
  /// [`AppError::DiagnosticReport`] if the report cannot be put together.
  pub fn prepare_report(
    &self,
    app: &AppSummary,
    now: OffsetDateTime,
  ) -> Result<DiagnosticReportPreview, AppError> {
    let logs = self.logger.read_files()?;
    let report = report::bundle(&logs, &self.environment, app, &self.session, now)?;
    let preview = report.preview();
    *self.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(report));
    Ok(preview)
  }

  /// The report [`Self::prepare_report`] last prepared, exactly as the
  /// user saw it; it stays until the next one is prepared.
  ///
  /// # Errors
  ///
  /// [`AppError::NoDiagnosticReport`] if none was.
  pub fn prepared_report(&self) -> Result<Arc<Report>, AppError> {
    self
      .pending
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .clone()
      .ok_or(AppError::NoDiagnosticReport)
  }
}

/// Where a failure was recorded: the module, file and line of the code
/// that recorded it, as [`record!`] fills it in. Compiled in with the
/// code, they always match the build that wrote them; the environment
/// record's commit says which source that was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
  pub module: &'static str,
  pub file: &'static str,
  pub line: u32,
}

/// Records an [`AppError`] at the severity of its kind, from the calling
/// module: `record!("session failed", &error, "role" => label)`. The
/// message is short and constant; the specifics follow it as fields.
macro_rules! record {
  ($msg:expr, $error:expr $(, $key:literal => $value:expr)* $(,)?) => {
    $crate::diagnostics::record_at(
      $crate::diagnostics::Site { module: module_path!(), file: file!(), line: line!() },
      $msg,
      $error,
      &[$(($key, ::log::kv::Value::from($value))),*],
    )
  };
}
pub(crate) use record;

/// Records `error`, which happened as `msg` says (short and constant:
/// "command failed", "session failed"), at `site`, at the severity of its
/// kind; the specifics go in `ctx`. [`record!`] fills in the site.
pub fn record_at(site: Site, msg: &str, error: &AppError, ctx: &[(&str, Value<'_>)]) {
  record_to(log::logger(), site, msg, error, ctx);
}

fn record_to(logger: &dyn Log, site: Site, msg: &str, error: &AppError, ctx: &[(&str, Value<'_>)]) {
  let Some(severity) = error.kind().severity() else {
    return;
  };
  let kind = format!("{:?}", error.kind());
  let detail = match error.hidden_cause() {
    Some(cause) => format!("{} ({cause})", error.message()),
    None => error.message(),
  };
  let mut fields: Vec<(&str, Value<'_>)> = vec![
    ("error_kind", Value::from(kind.as_str())),
    ("error_detail", Value::from(detail.as_str())),
  ];
  if severity == Severity::Critical {
    fields.push(("critical", Value::from(true)));
  }
  fields.extend(ctx.iter().map(|(key, value)| (*key, value.to_owned())));
  let level = match severity {
    Severity::Warning => log::Level::Warn,
    Severity::Error | Severity::Critical => log::Level::Error,
  };
  logger.log(
    &log::Record::builder()
      .level(level)
      .target(site.module)
      .file(Some(site.file))
      .line(Some(site.line))
      .key_values(&fields)
      .args(format_args!("{msg}"))
      .build(),
  );
}

/// Characters of an interface error's message kept, at most.
const UI_MESSAGE_CHARS: usize = 500;

/// Lines of an interface error's stack trace kept, at most: where it
/// happened, not the whole way there.
const UI_STACK_LINES: usize = 8;

/// Records an error the interface ran into: an error, or a critical
/// failure when the interface could not go on. Its message and stack
/// trace are cut short, so that the interface cannot flood the log.
pub fn record_ui_error(error: &UiError) {
  record_ui_error_to(log::logger(), error);
}

fn record_ui_error_to(logger: &dyn Log, error: &UiError) {
  let (detail, stack) = ui_error_parts(error);
  let msg = if error.fatal {
    "interface stopped"
  } else {
    "interface error"
  };
  let mut fields: Vec<(&str, Value<'_>)> = vec![
    (
      "error_kind",
      Value::from(error.name.as_deref().unwrap_or("Error")),
    ),
    ("error_detail", Value::from(detail.as_str())),
  ];
  if let Some(stack) = &stack {
    fields.push(("error_backtrace", Value::from(stack.as_str())));
  }
  if error.fatal {
    fields.push(("critical", Value::from(true)));
  }
  logger.log(
    &log::Record::builder()
      .level(log::Level::Error)
      .target(UI_TARGET)
      .key_values(&fields)
      .args(format_args!("{msg}"))
      .build(),
  );
}

/// An interface error's message and the start of its stack trace, each
/// line cut short.
fn ui_error_parts(error: &UiError) -> (String, Option<String>) {
  let detail = error.message.chars().take(UI_MESSAGE_CHARS).collect();
  let stack = error.stack.as_ref().map(|stack| {
    stack
      .lines()
      .take(UI_STACK_LINES)
      .map(|line| {
        line
          .trim()
          .chars()
          .take(UI_MESSAGE_CHARS)
          .collect::<String>()
      })
      .collect::<Vec<_>>()
      .join("\n")
  });
  (detail, stack)
}

/// Records every panic as a critical failure, where it happened, why and
/// the way there, before the default hook runs (a release build then
/// aborts): a crash leaves its cause in the log.
pub fn log_panics() {
  let previous = panic::take_hook();
  panic::set_hook(Box::new(move |info| {
    let why = info
      .payload()
      .downcast_ref::<&str>()
      .map(|why| (*why).to_owned())
      .or_else(|| info.payload().downcast_ref::<String>().cloned())
      .unwrap_or_else(|| "no message".to_owned());
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("unnamed");
    // A release build has no symbols (`strip`): there, where it happened
    // is the panic's location alone.
    let backtrace = resolved_frames(&Backtrace::force_capture().to_string());
    let mut fields: Vec<(&str, Value<'_>)> = vec![
      ("critical", Value::from(true)),
      ("error_kind", Value::from("Panic")),
      ("error_detail", Value::from(why.as_str())),
      ("thread", Value::from(thread)),
    ];
    if let Some(backtrace) = &backtrace {
      fields.push(("error_backtrace", Value::from(backtrace.as_str())));
    }
    log::logger().log(
      &log::Record::builder()
        .level(log::Level::Error)
        .target(PANIC_TARGET)
        .file(info.location().map(panic::Location::file))
        .line(info.location().map(panic::Location::line))
        .key_values(&fields)
        .args(format_args!("panic"))
        .build(),
    );
    log::logger().flush();
    previous(info);
  }));
}

/// A backtrace as `std` prints it, one frame per line (`3: f at
/// src/x.rs:12:5`); `None` when no frame has a name.
fn resolved_frames(backtrace: &str) -> Option<String> {
  let mut frames: Vec<String> = Vec::new();
  for line in backtrace.lines().map(str::trim) {
    if let Some(place) = line.strip_prefix("at ") {
      if let Some(frame) = frames.last_mut() {
        frame.push_str(" at ");
        frame.push_str(place);
      }
    } else if !line.is_empty() {
      frames.push(line.to_owned());
    }
  }
  let named = frames.iter().any(|frame| {
    frame
      .split_once(": ")
      .is_some_and(|(_, name)| !name.starts_with("0x") && !name.starts_with('<'))
  });
  named.then(|| frames.join("\n"))
}

#[cfg(test)]
mod tests {
  use std::fmt::Write as _;

  use serde_json::{Value as Json, json};

  use super::*;
  use crate::error::AppErrorKind;

  /// A logger writing to a directory of its own, and what it wrote.
  struct Written {
    dir: tempfile::TempDir,
    logger: DiagnosticLogger,
  }

  impl Written {
    fn new() -> Self {
      let dir = tempfile::tempdir().unwrap();
      let files = LogFiles::open(dir.path(), "{}".to_owned(), MAX_FILE_BYTES, KEPT_FILES).unwrap();
      let filter = Filter {
        steps: &[],
        quiet: &[],
      };
      let logger = DiagnosticLogger::new(filter, Arc::from("S"), files);
      Self { dir, logger }
    }

    /// The records written, the header left out.
    fn records(&self) -> Vec<Json> {
      std::fs::read_to_string(self.dir.path().join(files::CURRENT))
        .unwrap()
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
    }
  }

  #[test]
  fn a_failure_is_recorded_with_its_kind_its_detail_and_where_it_was_seen() {
    let written = Written::new();
    let site = Site {
      module: "cmd::bluetooth",
      file: "crates/cmd/src/bluetooth.rs",
      line: 298,
    };

    record_to(
      &written.logger,
      site,
      "session failed",
      &AppError::Disconnected,
      &[("role", "sync-waiting".into())],
    );
    record_to(
      &written.logger,
      site,
      "command failed",
      &AppError::WrongPassword,
      &[],
    );

    let records = written.records();
    assert_eq!(records.len(), 1, "a wrong password is no failure");
    assert_eq!(records[0]["level"], "warning");
    assert_eq!(records[0]["target"], "cmd::bluetooth");
    assert_eq!(records[0]["msg"], "session failed");
    assert_eq!(records[0]["error"]["kind"], "BluetoothDisconnected");
    assert!(records[0]["error"]["detail"].is_string());
    assert_eq!(records[0]["ctx"], json!({ "role": "sync-waiting" }));
    assert_eq!(records[0]["src"], "crates/cmd/src/bluetooth.rs:298");
  }

  #[test]
  fn an_interface_that_stopped_is_a_critical_failure() {
    let written = Written::new();

    record_ui_error_to(
      &written.logger,
      &UiError {
        name: Some("TypeError".to_owned()),
        message: "x is undefined".to_owned(),
        stack: Some("at f (index.js:1:1)\nat g (index.js:2:1)".to_owned()),
        fatal: true,
      },
    );

    let records = written.records();
    assert_eq!(records[0]["level"], "critical");
    assert_eq!(records[0]["target"], UI_TARGET);
    assert_eq!(records[0]["msg"], "interface stopped");
    assert_eq!(
      records[0]["error"],
      json!({
        "kind": "TypeError",
        "detail": "x is undefined",
        "backtrace": ["at f (index.js:1:1)", "at g (index.js:2:1)"]
      })
    );
  }

  #[test]
  fn what_the_user_typed_is_no_failure() {
    assert_eq!(AppErrorKind::WrongPassword.severity(), None);
    assert_eq!(AppErrorKind::MistypedInvitationCode.severity(), None);
  }

  #[test]
  fn failures_are_graded_by_who_is_at_fault_and_what_is_at_risk() {
    assert_eq!(
      AppErrorKind::BluetoothDisconnected.severity(),
      Some(Severity::Warning)
    );
    assert_eq!(AppErrorKind::FileWrite.severity(), Some(Severity::Error));
    assert_eq!(
      AppErrorKind::VaultDecode.severity(),
      Some(Severity::Critical)
    );
  }

  #[test]
  fn an_interface_error_keeps_its_message_and_where_it_happened_cut_short() {
    let stack = (1..=20).fold(String::new(), |mut stack, n| {
      let _ = writeln!(stack, "  at f{n} (index.js:{n}:1)");
      stack
    });
    let error = UiError {
      name: Some("TypeError".to_owned()),
      message: "x".repeat(2 * UI_MESSAGE_CHARS),
      stack: Some(stack),
      fatal: false,
    };

    let (detail, stack) = ui_error_parts(&error);

    assert_eq!(detail.len(), UI_MESSAGE_CHARS);
    let stack = stack.unwrap();
    let lines: Vec<&str> = stack.lines().collect();
    assert_eq!(lines.len(), UI_STACK_LINES);
    assert_eq!(lines[0], "at f1 (index.js:1:1)");
  }

  #[test]
  fn a_backtrace_keeps_one_line_per_frame() {
    let backtrace = "   0: std::backtrace::Backtrace::create\n             at /rustc/abc/library/std/src/backtrace.rs:331:13\n   1: cmd::vault::open\n             at ./crates/cmd/src/vault.rs:12:5\n";

    assert_eq!(
      resolved_frames(backtrace).unwrap(),
      "0: std::backtrace::Backtrace::create at /rustc/abc/library/std/src/backtrace.rs:331:13\n\
       1: cmd::vault::open at ./crates/cmd/src/vault.rs:12:5"
    );
  }

  #[test]
  fn a_backtrace_without_symbols_is_left_out() {
    assert_eq!(resolved_frames("   0: 0x55d1c2a3\n   1: <unknown>\n"), None);
  }
}
