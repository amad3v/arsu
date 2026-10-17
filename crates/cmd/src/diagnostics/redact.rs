//! What the diagnostic log must never hold, masked on its way in and
//! again on its way out.
//!
//! The first defence is that nothing logs such things: errors name what
//! failed, not what it held. This is the second, for whatever slips
//! through, from Arsu or from a library: every line passes here.

use std::{
  borrow::Cow,
  fmt::{self, Display},
  sync::LazyLock,
};

use regex::{Captures, Regex};

/// What is masked, and with what. Order matters: a URI goes as a whole
/// before its parts could match anything else.
static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
  [
    // An account's URI carries its secret.
    (r"otpauth(?:-migration)?://\S+", "otpauth://[masked]"),
    // An invitation's QR code text carries the pairing secret.
    (r"ARSU-INVITE:\S+", "ARSU-INVITE:[masked]"),
    // A typed invitation code: groups of five, dash-separated.
    (r"\b(?:[0-9A-Z]{5}-){4,}[0-9A-Z]{1,5}\b", "[code]"),
    // A TOTP secret in base32, as account backups hold them.
    (r"\b[A-Z2-7]{16,}=*", "[secret]"),
    // Account names are often email addresses.
    (r"[\w.+-]+@[\w-]+\.[\w.-]+", "[email]"),
    // A path in someone's home names them.
    (r"/home/[^/\s]+", "~"),
  ]
  .into_iter()
  .map(|(pattern, mask)| (Regex::new(pattern).expect("a valid pattern"), mask))
  .collect()
});

/// A Bluetooth address identifies a device for good: only its last two
/// bytes stay, enough to tell two devices apart in one log.
static ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"\b(?:[0-9A-Fa-f]{2}:){4}([0-9A-Fa-f]{2}:[0-9A-Fa-f]{2})\b").expect("a valid pattern")
});

/// This user's home directory, wherever it is (`/home/<user>` is masked
/// by [`RULES`] too; not every system keeps homes there): it names them.
static HOME: LazyLock<Option<Regex>> = LazyLock::new(|| {
  std::env::home_dir()
    .and_then(|home| home.to_str().map(home_pattern))
    .flatten()
});

/// A pattern for the home directory `home`, as a whole path component;
/// none for the root directory, or no directory.
fn home_pattern(home: &str) -> Option<Regex> {
  let home = home.trim_end_matches('/');
  if home.is_empty() {
    return None;
  }
  Regex::new(&format!(r"{}(?:/|\b|$)", regex::escape(home))).ok()
}

/// `text`, with everything the log must not hold masked.
#[must_use]
pub fn redact(text: &str) -> Cow<'_, str> {
  redact_with(text, HOME.as_ref())
}

fn redact_with<'t>(text: &'t str, home: Option<&Regex>) -> Cow<'t, str> {
  let mut text = Cow::Borrowed(text);
  if let Some(home) = home
    && let Cow::Owned(masked) =
      home.replace_all(
        &text,
        |found: &Captures<'_>| {
          if found[0].ends_with('/') { "~/" } else { "~" }
        },
      )
  {
    text = Cow::Owned(masked);
  }
  for (pattern, mask) in RULES.iter() {
    if let Cow::Owned(masked) = pattern.replace_all(&text, *mask) {
      text = Cow::Owned(masked);
    }
  }
  if let Cow::Owned(masked) = ADDRESS.replace_all(&text, |found: &Captures<'_>| {
    format!("**:**:**:**:{}", &found[1])
  }) {
    text = Cow::Owned(masked);
  }
  text
}

/// A Bluetooth address that prints masked as the log shows it
/// (`**:**:**:**:9A:BC`), for a log field: `address:% = MaskedAddress(…)`.
/// What does not read as an address prints as `[address]`.
#[derive(Debug, Clone, Copy)]
pub struct MaskedAddress<T>(pub T);

impl<T: Display> Display for MaskedAddress<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let address = self.0.to_string();
    match ADDRESS.captures(&address) {
      Some(found) if found[0].len() == address.len() => write!(f, "**:**:**:**:{}", &found[1]),
      _ => f.write_str("[address]"),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ordinary_text_passes_unchanged() {
    let line = "sync (waiting) session: this device is 2AE9 EDFF 3D4F A2B4, 2 trusted devices";

    assert!(matches!(redact(line), Cow::Borrowed(_)));
  }

  #[test]
  fn account_uris_lose_everything_after_the_scheme() {
    assert_eq!(
      redact("bad uri otpauth://totp/Bank:alice?secret=JBSWY3DPEHPK3PXP&issuer=Bank here"),
      "bad uri otpauth://[masked] here"
    );
    assert_eq!(
      redact("otpauth-migration://offline?data=CjEKCkhlbGxvId"),
      "otpauth://[masked]"
    );
  }

  #[test]
  fn invitations_are_masked_in_either_form() {
    assert_eq!(
      redact("read ARSU-INVITE:0123456789ABCDEF"),
      "read ARSU-INVITE:[masked]"
    );
    assert_eq!(
      redact("code ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567-89ABC-DEFGH-JKMNP-QRSTV-WXYZ0 typed"),
      "code [code] typed"
    );
  }

  #[test]
  fn base32_secrets_are_masked() {
    assert_eq!(
      redact("secret JBSWY3DPEHPK3PXP rejected"),
      "secret [secret] rejected"
    );
  }

  #[test]
  fn email_addresses_are_masked() {
    assert_eq!(
      redact("duplicate of alice.smith+2fa@example.co.uk"),
      "duplicate of [email]"
    );
  }

  #[test]
  fn home_directories_lose_their_owner() {
    assert_eq!(
      redact("could not write /home/alice/Documents/arsu-sync.arsu: Permission denied"),
      "could not write ~/Documents/arsu-sync.arsu: Permission denied"
    );
  }

  #[test]
  fn the_home_directory_is_masked_wherever_it_is() {
    let home = home_pattern("/srv/users/alice/").unwrap();

    assert_eq!(
      redact_with(
        "the vault at /srv/users/alice/.local/share/arsu/vault",
        Some(&home)
      ),
      "the vault at ~/.local/share/arsu/vault"
    );
    assert_eq!(redact_with("cd /srv/users/alice", Some(&home)), "cd ~");
    assert_eq!(
      redact_with("/srv/users/alicette/x", Some(&home)),
      "/srv/users/alicette/x",
      "another directory whose name starts the same"
    );
    assert!(home_pattern("/").is_none());
  }

  #[test]
  fn bluetooth_addresses_keep_their_last_two_bytes() {
    assert_eq!(
      redact("connecting to 12:34:56:78:9A:bc"),
      "connecting to **:**:**:**:9A:bc"
    );
  }

  #[test]
  fn an_address_field_prints_masked() {
    assert_eq!(
      MaskedAddress("12:34:56:78:9A:BC").to_string(),
      "**:**:**:**:9A:BC"
    );
    assert_eq!(MaskedAddress("my-computer").to_string(), "[address]");
  }

  #[test]
  fn fingerprints_and_kinds_stay_readable() {
    let line = "command failed: BluetoothDisconnected: the other device (7F8B A23D 7AB1 F095) left";

    assert_eq!(redact(line), line);
  }
}
