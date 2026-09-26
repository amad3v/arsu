//! The two clocks the app reads: wall-clock time for OTP codes and
//! timestamps, and boot time for measuring how long the user has been
//! away.

use std::{
  ops::Add,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

use rustix::time::{ClockId, clock_gettime};

use crate::error::AppError;

/// The current Unix time in seconds, which TOTP codes are computed from.
///
/// # Errors
///
/// Returns [`AppError::SystemClock`] if the clock is set before 1970.
pub fn unix_now() -> Result<u64, AppError> {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|since_epoch| since_epoch.as_secs())
    .map_err(AppError::SystemClock)
}

/// A point in time on Linux's `CLOCK_BOOTTIME`: time since boot,
/// *including* time spent suspended.
///
/// `std::time::Instant` uses `CLOCK_MONOTONIC`, which stops while the
/// machine sleeps — idle time measured with it would not count a night
/// with the lid closed, and the vault would still be unlocked in the
/// morning. `CLOCK_BOOTTIME` is also immune to wall-clock changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BootInstant(Duration);

impl BootInstant {
  /// # Panics
  ///
  /// Never in practice: the kernel does not report negative time since
  /// boot.
  #[must_use]
  pub fn now() -> Self {
    let since_boot = Duration::try_from(clock_gettime(ClockId::Boottime))
      .expect("CLOCK_BOOTTIME is never negative");
    Self(since_boot)
  }

  /// How long after `earlier` this is; zero if it is not after it.
  #[must_use]
  pub fn saturating_duration_since(self, earlier: Self) -> Duration {
    self.0.saturating_sub(earlier.0)
  }
}

impl Add<Duration> for BootInstant {
  type Output = Self;

  fn add(self, duration: Duration) -> Self {
    Self(self.0 + duration)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn boot_time_moves_forward() {
    let earlier = BootInstant::now();
    let later = BootInstant::now();
    assert!(later >= earlier);
    assert_eq!(earlier.saturating_duration_since(later), Duration::ZERO);
    assert_eq!(
      (earlier + Duration::from_secs(90)).saturating_duration_since(earlier),
      Duration::from_secs(90)
    );
  }
}
