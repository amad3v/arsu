//! The app's logger: it keeps
//! failures, and the steps that led to them.
//!
//! * A warning, an error or a critical failure is written at once, with
//!   the steps (routine `info` records of the step targets) that came
//!   before it in the last few minutes inside it: the log holds failures
//!   only.
//! * A step is kept in memory only, the latest [`STEPS_KEPT`]: carried by
//!   the next failure, or forgotten.
//! * Nothing below `info` is kept, and some targets are heard from only
//!   at their errors (a library that warns of nothing that matters).

use std::{
  collections::VecDeque,
  sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
  },
};

use log::{Log, Metadata, Record};
use serde::Serialize;
use time::{Duration, OffsetDateTime};

use super::{
  environment::Environment,
  event::{Event, Level, Step, timestamp},
  files::{LogFile, LogFiles},
};
use crate::error::AppError;

/// Steps kept in memory at most.
pub const STEPS_KEPT: usize = 30;

/// How long a step stays relevant: older ones are not written.
pub const STEP_MAX_AGE: Duration = Duration::minutes(5);

/// Which records the logger hears.
#[derive(Debug, Clone, Copy)]
pub struct Filter {
  /// Targets whose `info` records are steps (`cmd` covers `cmd::…`).
  pub steps: &'static [&'static str],
  /// Targets heard from only at their errors.
  pub quiet: &'static [&'static str],
}

impl Filter {
  fn hears(&self, level: log::Level, target: &str) -> bool {
    let within = |prefixes: &[&str]| {
      prefixes.iter().any(|prefix| {
        target == *prefix
          || target
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with("::"))
      })
    };
    match level {
      log::Level::Error => true,
      log::Level::Warn => !within(self.quiet),
      log::Level::Info => within(self.steps),
      log::Level::Debug | log::Level::Trace => false,
    }
  }
}

/// The environment record that heads each file and each app run.
#[derive(Serialize)]
struct Header<'a> {
  ts: String,
  #[serde(rename = "type")]
  kind: &'static str,
  session: &'a str,
  #[serde(flatten)]
  environment: &'a Environment,
}

/// The logger: the log files, the steps in memory, and the run's
/// numbering.
pub struct DiagnosticLogger {
  filter: Filter,
  session: Arc<str>,
  next: AtomicU64,
  files: Mutex<LogFiles>,
  steps: Mutex<VecDeque<(OffsetDateTime, Step)>>,
}

impl DiagnosticLogger {
  /// The logger for the app run `session`, writing to `files`, opened
  /// with [`header`].
  #[must_use]
  pub fn new(filter: Filter, session: Arc<str>, files: LogFiles) -> Self {
    Self {
      filter,
      session,
      next: AtomicU64::new(1),
      files: Mutex::new(files),
      steps: Mutex::new(VecDeque::new()),
    }
  }

  /// Takes `record` as it happened at `now`.
  pub fn take(&self, record: &Record<'_>, now: OffsetDateTime) {
    if !self.filter.hears(record.level(), record.target()) {
      return;
    }
    let seq = self.next.fetch_add(1, Ordering::Relaxed);
    let mut event = Event::from_record(record, now, Arc::clone(&self.session), seq);
    if event.level == Level::Info {
      self.remember(now, Step::from(event));
      return;
    }
    event.steps = lock(&self.steps)
      .drain(..)
      .filter(|(at, _)| now - *at <= STEP_MAX_AGE)
      .map(|(_, step)| step)
      .collect();
    // A log that cannot be written has nowhere to say so.
    let _ = lock(&self.files).write(&event.to_line());
  }

  /// The log files, oldest first, as written so far: read under the
  /// logger's lock, so that none rotates meanwhile.
  ///
  /// # Errors
  ///
  /// [`AppError::FileRead`] if one cannot be read.
  pub fn read_files(&self) -> Result<Vec<LogFile>, AppError> {
    lock(&self.files).read_all()
  }

  fn remember(&self, now: OffsetDateTime, step: Step) {
    let mut steps = lock(&self.steps);
    if let Some((at, last)) = steps.back_mut()
      && last.same_as(&step)
    {
      last.repeat = last.repeat.saturating_add(1);
      last.ts = step.ts;
      *at = now;
      return;
    }
    if steps.len() == STEPS_KEPT {
      steps.pop_front();
    }
    steps.push_back((now, step));
  }
}

impl Log for DiagnosticLogger {
  fn enabled(&self, metadata: &Metadata<'_>) -> bool {
    self.filter.hears(metadata.level(), metadata.target())
  }

  fn log(&self, record: &Record<'_>) {
    self.take(record, OffsetDateTime::now_utc());
  }

  fn flush(&self) {
    let _ = lock(&self.files).flush();
  }
}

/// The environment record heading the log files of the app run
/// `session`, made at `now`.
#[must_use]
pub fn header(environment: &Environment, session: &str, now: OffsetDateTime) -> String {
  serde_json::to_string(&Header {
    ts: timestamp(now),
    kind: "env",
    session,
    environment,
  })
  .unwrap_or_default()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  // A poisoned lock still holds valid records.
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
  use std::fs;

  use serde_json::{Value as Json, json};
  use time::macros::datetime;

  use super::*;
  use crate::diagnostics::{environment::Environment, files::CURRENT};

  const NOW: OffsetDateTime = datetime!(2026-10-11 14:03:22.114 UTC);

  const FILTER: Filter = Filter {
    steps: &["cmd", "command"],
    quiet: &["tao"],
  };

  fn environment() -> Environment {
    Environment::detect("Arsu", "1.2.0", "abc123", "WebKitGTK 2.46".to_owned())
  }

  struct Fixture {
    dir: tempfile::TempDir,
    logger: DiagnosticLogger,
  }

  impl Fixture {
    fn new() -> Self {
      let dir = tempfile::tempdir().unwrap();
      let header = header(&environment(), "S", NOW);
      let files = LogFiles::open(dir.path(), header, 1_000_000, 5).unwrap();
      let logger = DiagnosticLogger::new(FILTER, Arc::from("S"), files);
      Self { dir, logger }
    }

    fn log(&self, level: log::Level, target: &str, message: &str, at: OffsetDateTime) {
      self.logger.take(
        &Record::builder()
          .level(level)
          .target(target)
          .args(format_args!("{message}"))
          .build(),
        at,
      );
    }

    fn records(&self) -> Vec<Json> {
      fs::read_to_string(self.dir.path().join(CURRENT))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
    }
  }

  #[test]
  fn a_run_that_records_nothing_writes_nothing() {
    let fixture = Fixture::new();
    fixture.log(log::Level::Info, "cmd", "a step", NOW);

    assert!(fixture.records().is_empty());
  }

  #[test]
  fn a_failure_follows_what_the_app_and_device_are() {
    let fixture = Fixture::new();
    fixture.log(log::Level::Warn, "cmd", "failure", NOW);

    let records = fixture.records();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["type"], "env");
    assert_eq!(records[0]["session"], "S");
    assert_eq!(records[0]["app"]["version"], "1.2.0");
    assert_eq!(records[0]["app"]["commit"], "abc123");
    assert_eq!(records[0]["runtime"]["webview"], "WebKitGTK 2.46");
    assert!(records[0]["os"]["name"].is_string());
    assert_eq!(records[1]["msg"], "failure");
  }

  #[test]
  fn a_failure_carries_the_recent_steps_once_and_the_log_holds_no_step() {
    let fixture = Fixture::new();
    fixture.log(
      log::Level::Info,
      "cmd",
      "too old",
      NOW - Duration::minutes(10),
    );
    fixture.log(
      log::Level::Info,
      "command",
      "unlock_vault",
      NOW - Duration::seconds(2),
    );
    fixture.log(
      log::Level::Info,
      "command",
      "unlock_vault",
      NOW - Duration::seconds(1),
    );

    fixture.log(log::Level::Error, "arsu", "command failed", NOW);
    fixture.log(log::Level::Error, "arsu", "command failed", NOW);

    let records = fixture.records();
    let levels: Vec<&str> = records[1..]
      .iter()
      .map(|record| record["level"].as_str().unwrap())
      .collect();
    assert_eq!(levels, ["error", "error"]);
    assert_eq!(
      records[1]["steps"],
      json!([{
        "ts": "2026-10-11T14:03:21.114Z",
        "target": "command",
        "msg": "unlock_vault",
        "repeat": 2
      }])
    );
    assert!(
      records[2].get("steps").is_none(),
      "each step goes with one failure"
    );
  }

  #[test]
  fn records_are_numbered_in_order_counting_the_steps() {
    let fixture = Fixture::new();
    fixture.log(log::Level::Info, "cmd", "a", NOW);
    fixture.log(log::Level::Warn, "cmd", "b", NOW);
    fixture.log(log::Level::Info, "cmd", "c", NOW);
    fixture.log(log::Level::Warn, "cmd", "d", NOW);

    let seqs: Vec<u64> = fixture.records()[1..]
      .iter()
      .map(|record| record["seq"].as_u64().unwrap())
      .collect();

    assert_eq!(seqs, [2, 4]);
  }

  #[test]
  fn only_failures_and_the_steps_of_step_targets_are_heard() {
    assert!(FILTER.hears(log::Level::Info, "cmd::bluetooth"));
    assert!(!FILTER.hears(log::Level::Info, "cmdline"));
    assert!(!FILTER.hears(log::Level::Info, "zbus"));
    assert!(!FILTER.hears(log::Level::Debug, "cmd"));
    assert!(FILTER.hears(log::Level::Warn, "zbus"));
    assert!(!FILTER.hears(log::Level::Warn, "tao::platform"));
    assert!(FILTER.hears(log::Level::Error, "tao::platform"));
  }

  #[test]
  fn only_the_latest_steps_are_kept() {
    let fixture = Fixture::new();
    for n in 0..STEPS_KEPT + 5 {
      fixture.log(log::Level::Info, "cmd", &format!("step {n}"), NOW);
    }
    fixture.log(log::Level::Warn, "cmd", "failure", NOW);

    let steps = fixture.records()[1]["steps"].as_array().unwrap().clone();

    assert_eq!(steps.len(), STEPS_KEPT);
    assert_eq!(steps[0]["msg"], "step 5");
  }
}
