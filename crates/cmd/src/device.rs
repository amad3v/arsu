//! Whether the phone is rooted: a root app can read any app's memory and
//! files, the unlocked vault's included, so the user is told before the
//! vault is opened there (see `AppState::device_check`).
//!
//! The check is a best effort, not a guarantee: it finds the `su` binary
//! that a root manager (Magisk, `SuperSU`) puts on the path, but a root
//! hidden from apps (Magisk's deny list, `KernelSU`, `APatch`) passes
//! unseen. It never stops the app; it only asks for the user's consent.

#[cfg(any(target_os = "android", test))]
use std::{
  ffi::OsStr,
  path::{Path, PathBuf},
};

/// Where root managers put `su`, or their own app, beyond the `PATH`.
#[cfg(any(target_os = "android", test))]
const ROOT_TRACES: &[&str] = &[
  "/system/bin/su",
  "/system_ext/bin/su",
  "/product/bin/su",
  "/odm/bin/su",
  "/vendor/xbin/su",
  "/system/xbin/su",
  "/system/sbin/su",
  "/system/su",
  "/system/bin/.ext/su",
  "/system/usr/we-need-root/su",
  "/sbin/su",
  "/su/bin/su",
  "/vendor/bin/su",
  "/debug_ramdisk/su",
  "/cache/su",
  "/data/local/su",
  "/data/local/bin/su",
  "/data/local/xbin/su",
  "/system/app/Superuser.apk",
  "/system/app/SuperSU.apk",
];

/// Whether this device looks rooted. Always `false` off Android: a
/// desktop's administrator is its user.
#[must_use]
pub fn rooted() -> bool {
  #[cfg(target_os = "android")]
  {
    root_traces_found(
      |path| path.try_exists().unwrap_or(false),
      std::env::var_os("PATH").as_deref(),
    )
  }
  #[cfg(not(target_os = "android"))]
  {
    false
  }
}

/// Whether any of [`ROOT_TRACES`], or an `su` in a directory of `path_var`
/// (the `PATH`), `exists`. A path that cannot be looked at (the app is
/// denied it) counts as absent.
#[cfg(any(target_os = "android", test))]
fn root_traces_found(exists: impl Fn(&Path) -> bool, path_var: Option<&OsStr>) -> bool {
  let in_path = path_var
    .into_iter()
    .flat_map(std::env::split_paths)
    .filter(|dir| dir.is_absolute())
    .map(|dir| dir.join("su"));
  ROOT_TRACES
    .iter()
    .map(PathBuf::from)
    .chain(in_path)
    .any(|path| exists(&path))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_known_su_location_is_root() {
    assert!(root_traces_found(
      |path| path == Path::new("/system/xbin/su"),
      None
    ));
  }

  #[test]
  fn an_su_on_the_path_is_root() {
    let path_var = OsStr::new("/apex/bin:/odd/place");
    assert!(root_traces_found(
      |path| path == Path::new("/odd/place/su"),
      Some(path_var)
    ));
  }

  #[test]
  fn no_su_anywhere_is_not_root() {
    let path_var = OsStr::new("/system/bin:relative/dir");
    assert!(!root_traces_found(|_| false, Some(path_var)));
    // A relative PATH entry is never looked at.
    assert!(!root_traces_found(
      |path| path == Path::new("relative/dir/su"),
      Some(path_var)
    ));
  }

  #[test]
  fn the_desktop_is_never_rooted() {
    #[cfg(not(target_os = "android"))]
    assert!(!rooted());
  }
}
