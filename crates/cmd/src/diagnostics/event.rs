//! One diagnostic record, as a line of JSON.
//!
//! A record is built from a `log` record: its message is a short constant
//! saying what happened, and what varies goes into structured fields
//! (`log::warn!(address:% = …, mtu = 512; "connected")`), so that the
//! same problem reads the same in every report. Two kinds of field are
//! read specially:
//!
//! * `critical = true` makes an error a critical failure;
//! * `error_kind`, `error_detail` and `error_backtrace` make up the
//!   record's `error`.
//!
//! Every text in a record is masked ([`redact`]) as the record is built:
//! nothing unmasked ever reaches a file.

use std::{fmt, sync::Arc};

use log::kv::{self, Key, Value, VisitSource, VisitValue};
use serde::Serialize;
use serde_json::{Map, Value as Json};
use time::{OffsetDateTime, macros::format_description};

use super::redact::redact;

/// Frames of a backtrace kept, at most.
const BACKTRACE_FRAMES: usize = 40;

/// How bad a record is. `Info` is a routine step, written only as the
/// context of a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
  Info,
  Warning,
  Error,
  Critical,
}

/// What went wrong, for a failure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ErrorField {
  /// Its kind, the name the interface knows it by.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kind: Option<String>,
  /// Its message and every cause.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub detail: Option<String>,
  /// Where it happened, for a crash: one frame per entry.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub backtrace: Option<Vec<String>>,
}

/// One record of the diagnostic log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
  /// When, in UTC (RFC 3339, milliseconds).
  pub ts: String,
  pub level: Level,
  /// Where it comes from: a module, or a component such as `command`.
  pub target: String,
  /// What happened: short and constant.
  pub msg: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<ErrorField>,
  /// The specifics.
  #[serde(skip_serializing_if = "Map::is_empty")]
  pub ctx: Map<String, Json>,
  /// The source file and line that logged it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub src: Option<String>,
  /// The app run it belongs to.
  pub session: Arc<str>,
  /// Its place in the run, counting every record (written or not): the
  /// order survives a clock set back.
  pub seq: u64,
  /// For a failure, the steps that led to it, oldest first.
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub steps: Vec<Step>,
}

/// A routine step (an `info` record), as a failure carries it: what
/// happened and when, nothing that the failure says already.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
  pub ts: String,
  pub target: String,
  pub msg: String,
  #[serde(skip_serializing_if = "Map::is_empty")]
  pub ctx: Map<String, Json>,
  /// How many times in a row it happened, when more than once: a step
  /// repeated is counted, not repeated.
  #[serde(skip_serializing_if = "is_once")]
  pub repeat: u32,
}

#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "serde's skip_serializing_if passes a reference"
)]
fn is_once(times: &u32) -> bool {
  *times == 1
}

impl Event {
  /// `record`, at `level`, as the `seq`th record of `session`, made at
  /// `now`; masked throughout.
  #[must_use]
  pub fn from_record(
    record: &log::Record<'_>,
    now: OffsetDateTime,
    session: Arc<str>,
    seq: u64,
  ) -> Self {
    let mut fields = Fields::default();
    // A field that cannot be read is left out; the rest still are.
    let _ = record.key_values().visit(&mut fields);
    let level = match record.level() {
      log::Level::Error if fields.critical => Level::Critical,
      log::Level::Error => Level::Error,
      log::Level::Warn => Level::Warning,
      log::Level::Info | log::Level::Debug | log::Level::Trace => Level::Info,
    };
    Self {
      ts: timestamp(now),
      level,
      target: record.target().to_owned(),
      msg: redact(&record.args().to_string()).into_owned(),
      error: (fields.error != ErrorField::default()).then_some(fields.error),
      ctx: fields.ctx,
      src: record
        .file()
        .map(|file| format!("{}:{}", short_source(file), record.line().unwrap_or(0))),
      session,
      seq,
      steps: Vec::new(),
    }
  }

  /// The record as one line of JSON.
  #[must_use]
  pub fn to_line(&self) -> String {
    // Strings, numbers and maps of them always serialize.
    serde_json::to_string(self).unwrap_or_default()
  }
}

impl From<Event> for Step {
  fn from(event: Event) -> Self {
    Self {
      ts: event.ts,
      target: event.target,
      msg: event.msg,
      ctx: event.ctx,
      repeat: 1,
    }
  }
}

impl Step {
  /// Whether `other` is the same step as this one, for counting repeats.
  #[must_use]
  pub fn same_as(&self, other: &Self) -> bool {
    self.target == other.target && self.msg == other.msg && self.ctx == other.ctx
  }
}

/// A time as records give it: RFC 3339 in UTC, to the millisecond.
#[must_use]
pub fn timestamp(at: OffsetDateTime) -> String {
  at.format(format_description!(
    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
  ))
  .unwrap_or_default()
}

/// A source path as a report shows it: from the crate on, without where
/// the build machine kept it.
fn short_source(file: &str) -> &str {
  const REGISTRY: &str = "/registry/src/";
  match file.find(REGISTRY) {
    // `…/registry/src/<index>/<crate>-<version>/src/…`: from the crate on.
    Some(at) => {
      let after = &file[at + REGISTRY.len()..];
      after.split_once('/').map_or(after, |(_index, rest)| rest)
    }
    None => file,
  }
}

/// A record's fields, sorted into its context, its error and whether it
/// is critical.
#[derive(Default)]
struct Fields {
  critical: bool,
  error: ErrorField,
  ctx: Map<String, Json>,
}

impl<'kvs> VisitSource<'kvs> for Fields {
  fn visit_pair(&mut self, key: Key<'kvs>, value: Value<'kvs>) -> Result<(), kv::Error> {
    let json = to_json(&value);
    match key.as_str() {
      "critical" => self.critical = json == Json::Bool(true),
      "error_kind" => self.error.kind = Some(text(&json)),
      "error_detail" => self.error.detail = Some(text(&json)),
      "error_backtrace" => {
        self.error.backtrace = Some(
          text(&json)
            .lines()
            .map(str::trim)
            .filter(|frame| !frame.is_empty())
            .take(BACKTRACE_FRAMES)
            .map(str::to_owned)
            .collect(),
        );
      }
      key => {
        self.ctx.insert(key.to_owned(), json);
      }
    }
    Ok(())
  }
}

/// A field's value as JSON: numbers and booleans as they are, anything
/// else as masked text.
fn to_json(value: &Value<'_>) -> Json {
  let mut json = JsonValue(Json::Null);
  if value.visit(&mut json).is_err() {
    return Json::String(redact(&value.to_string()).into_owned());
  }
  json.0
}

fn text(json: &Json) -> String {
  match json {
    Json::String(text) => text.clone(),
    other => other.to_string(),
  }
}

struct JsonValue(Json);

impl VisitValue<'_> for JsonValue {
  fn visit_any(&mut self, value: Value<'_>) -> Result<(), kv::Error> {
    self.0 = Json::String(redact(&value.to_string()).into_owned());
    Ok(())
  }

  fn visit_null(&mut self) -> Result<(), kv::Error> {
    self.0 = Json::Null;
    Ok(())
  }

  fn visit_bool(&mut self, value: bool) -> Result<(), kv::Error> {
    self.0 = Json::Bool(value);
    Ok(())
  }

  fn visit_u64(&mut self, value: u64) -> Result<(), kv::Error> {
    self.0 = Json::from(value);
    Ok(())
  }

  fn visit_i64(&mut self, value: i64) -> Result<(), kv::Error> {
    self.0 = Json::from(value);
    Ok(())
  }

  fn visit_f64(&mut self, value: f64) -> Result<(), kv::Error> {
    self.0 = Json::from(value);
    Ok(())
  }

  fn visit_str(&mut self, value: &str) -> Result<(), kv::Error> {
    self.0 = Json::String(redact(value).into_owned());
    Ok(())
  }
}

impl fmt::Display for Level {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Self::Info => "info",
      Self::Warning => "warning",
      Self::Error => "error",
      Self::Critical => "critical",
    })
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;
  use time::macros::datetime;

  use super::*;

  const NOW: OffsetDateTime = datetime!(2026-10-11 14:03:22.114 UTC);

  fn session() -> Arc<str> {
    Arc::from("0199d7a2-0000-7000-8000-000000000000")
  }

  #[test]
  fn a_record_becomes_one_line_of_json_with_its_fields_in_context() {
    let kvs: [(&str, Value<'_>); 2] = [
      ("role", Value::from("sync (connecting)")),
      ("att_mtu", Value::from(517_u64)),
    ];
    let record = log::Record::builder()
      .level(log::Level::Warn)
      .target("cmd::bluetooth")
      .file(Some("crates/cmd/src/bluetooth.rs"))
      .line(Some(362))
      .key_values(&kvs)
      .args(format_args!("connected"))
      .build();

    let event = Event::from_record(&record, NOW, session(), 412);

    assert_eq!(
      serde_json::from_str::<Json>(&event.to_line()).unwrap(),
      json!({
        "ts": "2026-10-11T14:03:22.114Z",
        "level": "warning",
        "target": "cmd::bluetooth",
        "msg": "connected",
        "ctx": { "role": "sync (connecting)", "att_mtu": 517 },
        "src": "crates/cmd/src/bluetooth.rs:362",
        "session": "0199d7a2-0000-7000-8000-000000000000",
        "seq": 412
      })
    );
  }

  #[test]
  fn error_fields_make_the_error_and_critical_marks_the_level() {
    let kvs: [(&str, Value<'_>); 4] = [
      ("critical", Value::from(true)),
      ("error_kind", Value::from("Panic")),
      ("error_detail", Value::from("index out of bounds")),
      ("error_backtrace", Value::from("  0: a\n\n  1: b\n")),
    ];
    let record = log::Record::builder()
      .level(log::Level::Error)
      .target("arsu")
      .key_values(&kvs)
      .args(format_args!("panic"))
      .build();

    let event = Event::from_record(&record, NOW, session(), 1);

    assert_eq!(event.level, Level::Critical);
    assert!(event.ctx.is_empty());
    assert_eq!(
      event.error,
      Some(ErrorField {
        kind: Some("Panic".to_owned()),
        detail: Some("index out of bounds".to_owned()),
        backtrace: Some(vec!["0: a".to_owned(), "1: b".to_owned()]),
      })
    );
  }

  #[test]
  fn every_text_is_masked_as_the_record_is_made() {
    let address = "12:34:56:78:9A:BC";
    let kvs: [(&str, Value<'_>); 2] = [
      ("address", Value::from_display(&address)),
      ("error_detail", Value::from("could not write /home/alice/x")),
    ];
    let record = log::Record::builder()
      .level(log::Level::Error)
      .target("arsu")
      .key_values(&kvs)
      .args(format_args!(
        "read otpauth://totp/a?secret=JBSWY3DPEHPK3PXP"
      ))
      .build();

    let line = Event::from_record(&record, NOW, session(), 1).to_line();

    assert!(line.contains("**:**:**:**:9A:BC"), "{line}");
    assert!(line.contains("could not write ~/x"), "{line}");
    assert!(line.contains("otpauth://[masked]"), "{line}");
    assert!(!line.contains("alice") && !line.contains("JBSW"), "{line}");
  }

  #[test]
  fn a_dependencys_source_drops_where_the_build_kept_it() {
    assert_eq!(
      short_source("/home/u/.cargo/registry/src/index.crates.io-1949/blec-0.17.0/src/handler.rs"),
      "blec-0.17.0/src/handler.rs"
    );
    assert_eq!(short_source("crates/cmd/src/x.rs"), "crates/cmd/src/x.rs");
  }
}
