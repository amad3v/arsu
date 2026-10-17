//! What the app and the device are: the first record of every log file
//! and of every app run, and the report's `environment.json`. A bug report usually lacks this more
//! than it lacks log lines. No host name, user name or path: nothing that
//! names the user.

use serde::Serialize;

/// The app, the operating system, the device and the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Environment {
  pub app: App,
  pub os: Os,
  /// On a phone: who made it and which model it is.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub device: Option<Device>,
  /// The processor architecture, as Rust names it (`aarch64`, `x86_64`).
  pub arch: String,
  /// The system's language and region, as a BCP 47 tag (`en-GB`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub locale: Option<String>,
  pub runtime: Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct App {
  pub name: String,
  pub version: String,
  /// `release` or `debug`.
  pub build: &'static str,
  /// The commit it was built from, where CI built it.
  pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Os {
  /// `Linux` or `Android`.
  pub name: &'static str,
  /// The distribution's name and version, or Android's version.
  pub version: String,
  /// The kernel's release (Linux), or the API level (Android).
  pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
  pub maker: String,
  pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Runtime {
  pub tauri: String,
  pub webview: String,
}

impl Environment {
  /// This device's environment, for the app `name` at `version`, built
  /// from `commit`, its interface running in `webview`.
  #[must_use]
  pub fn detect(name: &str, version: &str, commit: &str, webview: String) -> Self {
    let (os, device) = platform();
    Self {
      app: App {
        name: name.to_owned(),
        version: version.to_owned(),
        build: if cfg!(debug_assertions) {
          "debug"
        } else {
          "release"
        },
        commit: commit.to_owned(),
      },
      os,
      device,
      arch: std::env::consts::ARCH.to_owned(),
      locale: locale(),
      runtime: Runtime {
        tauri: tauri::VERSION.to_owned(),
        webview,
      },
    }
  }
}

#[cfg(target_os = "linux")]
fn platform() -> (Os, Option<Device>) {
  let distribution = std::fs::read_to_string("/etc/os-release")
    .ok()
    .and_then(|release| {
      release.lines().find_map(|line| {
        line
          .strip_prefix("PRETTY_NAME=")
          .map(|name| name.trim_matches('"').to_owned())
      })
    })
    .unwrap_or_else(|| "unknown distribution".to_owned());
  let kernel = rustix::system::uname()
    .release()
    .to_string_lossy()
    .into_owned();
  (
    Os {
      name: "Linux",
      version: distribution,
      detail: kernel,
    },
    None,
  )
}

#[cfg(target_os = "android")]
fn platform() -> (Os, Option<Device>) {
  let properties = android_system_properties::AndroidSystemProperties::new();
  let read = |name: &str| properties.get(name).unwrap_or_else(|| "?".to_owned());
  (
    Os {
      name: "Android",
      version: read("ro.build.version.release"),
      detail: format!("API {}", read("ro.build.version.sdk")),
    },
    Some(Device {
      maker: read("ro.product.manufacturer"),
      model: read("ro.product.model"),
    }),
  )
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn platform() -> (Os, Option<Device>) {
  (
    Os {
      name: std::env::consts::OS,
      version: String::new(),
      detail: String::new(),
    },
    None,
  )
}

/// The system's locale, as a BCP 47 tag, as Android gives it.
#[cfg(not(target_os = "android"))]
fn locale() -> Option<String> {
  ["LC_ALL", "LC_MESSAGES", "LANG"]
    .into_iter()
    .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
    .and_then(|value| posix_to_bcp47(&value))
}

/// A POSIX locale (`en_GB.UTF-8@euro`) as a BCP 47 tag (`en-GB`): its
/// encoding and modifier left out. None for the `C` and `POSIX` locales,
/// which name no language.
#[cfg(not(target_os = "android"))]
fn posix_to_bcp47(locale: &str) -> Option<String> {
  let name = locale.split(['.', '@']).next().unwrap_or(locale);
  (!name.is_empty() && name != "C" && name != "POSIX").then(|| name.replace('_', "-"))
}

/// The system's locale.
#[cfg(target_os = "android")]
fn locale() -> Option<String> {
  let properties = android_system_properties::AndroidSystemProperties::new();
  ["persist.sys.locale", "ro.product.locale"]
    .into_iter()
    .find_map(|name| properties.get(name).filter(|value| !value.is_empty()))
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
  use super::*;

  #[test]
  fn a_posix_locale_reads_as_android_names_it() {
    assert_eq!(posix_to_bcp47("en_GB.UTF-8").as_deref(), Some("en-GB"));
    assert_eq!(posix_to_bcp47("de_DE@euro").as_deref(), Some("de-DE"));
    assert_eq!(posix_to_bcp47("fr").as_deref(), Some("fr"));
    assert_eq!(posix_to_bcp47("C.UTF-8"), None);
    assert_eq!(posix_to_bcp47("POSIX"), None);
  }
}
