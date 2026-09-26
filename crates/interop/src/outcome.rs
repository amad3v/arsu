//! Turning a backup file's entries into an [`ImportOutcome`]: one entry
//! at a time, so a bad entry is reported instead of failing the file,
//! and with duplicate detection against the vault and within the file.

use std::collections::HashSet;

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::value::RawValue;

use qr::{OtpKind, ParsedAccount, QrError};
use vault_core::{Entry, OtpConfig, SecretBytes};

/// The result of importing a backup file: the accounts to add, and the
/// entries that were left out, each with the reason.
#[derive(Debug, Default)]
pub struct ImportOutcome {
  pub imported: Vec<ParsedAccount>,
  /// Entries that are not valid accounts.
  pub skipped: Vec<SkippedEntry>,
  /// Valid accounts that are already in the vault, or that appeared
  /// earlier in the same file.
  pub duplicates: Vec<SkippedEntry>,
}

/// An entry that was not imported.
#[derive(Debug)]
pub struct SkippedEntry {
  /// How the source file names the entry, for telling the user which
  /// one; not a stable identifier.
  pub label: String,
  pub reason: SkipReason,
}

/// Why an entry was not imported. The messages are meant for the user.
#[derive(Debug, thiserror::Error)]
pub enum SkipReason {
  /// The entry is not a valid account (for example an unsupported OTP
  /// type or a secret that is not base32).
  #[error(transparent)]
  Invalid(#[from] QrError),
  /// The entry does not have the shape its format specifies.
  #[error("malformed entry: {0}")]
  Malformed(serde_json::Error),
  #[error("already in the vault")]
  AlreadyInVault,
  #[error("appears more than once in the file")]
  DuplicateInFile,
}

/// The accounts already in the vault, so an import can leave out the
/// ones it would add a second time.
///
/// Collect it from the vault's live entries:
/// `payload.active_entries().collect::<KnownAccounts>()`. A deleted
/// entry is not known, so importing the account again restores it.
#[derive(Debug, Default)]
pub struct KnownAccounts(HashSet<AccountKey>);

impl<'entry> FromIterator<&'entry Entry> for KnownAccounts {
  fn from_iter<I: IntoIterator<Item = &'entry Entry>>(entries: I) -> Self {
    Self(entries.into_iter().map(AccountKey::from_entry).collect())
  }
}

/// What makes two accounts the same one: issuer and account name (in
/// any letter case, ignoring surrounding whitespace), OTP scheme and
/// secret. Two accounts that differ only in secret are different
/// enrolments and are both kept. `Debug` is safe to derive:
/// [`SecretBytes`] redacts itself.
#[derive(Debug, PartialEq, Eq, Hash)]
struct AccountKey {
  issuer: String,
  account_label: String,
  kind: OtpKind,
  secret: SecretBytes,
}

impl AccountKey {
  fn new(issuer: Option<&str>, account_label: &str, otp: &OtpConfig) -> Self {
    let (kind, secret) = match otp {
      OtpConfig::Totp { secret, .. } => (OtpKind::Totp, secret),
      OtpConfig::Hotp { secret, .. } => (OtpKind::Hotp, secret),
    };
    Self {
      issuer: issuer.unwrap_or_default().trim().to_lowercase(),
      account_label: account_label.trim().to_lowercase(),
      kind,
      secret: secret.clone(),
    }
  }

  fn from_entry(entry: &Entry) -> Self {
    Self::new(entry.issuer.as_deref(), &entry.account_label, &entry.otp)
  }

  fn from_account(account: &ParsedAccount) -> Self {
    Self::new(account.issuer(), account.account_label(), account.otp())
  }
}

/// One entry of a backup file, as the format stores it.
pub(crate) trait SourceEntry: DeserializeOwned {
  /// How the file names the entry, if it names it at all.
  fn label(&self) -> Option<String>;

  /// Validates the entry as an account, through the same rules as
  /// every other way of adding one (see [`ParsedAccount::new`]).
  fn into_account(self) -> Result<ParsedAccount, QrError>;
}

/// Imports a backup file's entry list. Each entry is parsed and
/// validated on its own: one that fails is reported in `skipped` and the
/// rest still import.
pub(crate) fn import_entries<E: SourceEntry>(
  entries: &[&RawValue],
  known: &KnownAccounts,
) -> ImportOutcome {
  let mut sorter = Sorter::new(known);
  for (position, raw) in entries.iter().enumerate() {
    match serde_json::from_str::<E>(raw.get()) {
      Ok(entry) => {
        let label = entry.label().unwrap_or_else(|| numbered(position));
        sorter.place(label, entry.into_account().map_err(SkipReason::from));
      }
      Err(error) => sorter.place(
        fallback_label(raw, position),
        Err(SkipReason::Malformed(error)),
      ),
    }
  }
  sorter.outcome
}

/// "Issuer (account)", or whichever of the two is not blank.
pub(crate) fn display_label(issuer: Option<&str>, account: Option<&str>) -> Option<String> {
  fn non_blank(part: Option<&str>) -> Option<&str> {
    part.map(str::trim).filter(|part| !part.is_empty())
  }

  match (non_blank(issuer), non_blank(account)) {
    (Some(issuer), Some(account)) => Some(format!("{issuer} ({account})")),
    (Some(label), None) | (None, Some(label)) => Some(label.to_owned()),
    (None, None) => None,
  }
}

/// A label for an entry that did not parse: its `name` (both formats
/// have one) if that much can be read, else its position.
fn fallback_label(raw: &RawValue, position: usize) -> String {
  #[derive(Deserialize)]
  struct Named {
    name: String,
  }

  serde_json::from_str::<Named>(raw.get())
    .ok()
    .and_then(|named| display_label(None, Some(&named.name)))
    .unwrap_or_else(|| numbered(position))
}

fn numbered(position: usize) -> String {
  format!("entry {}", position + 1)
}

/// Sorts candidate entries into an [`ImportOutcome`].
struct Sorter<'known> {
  known: &'known KnownAccounts,
  seen: HashSet<AccountKey>,
  outcome: ImportOutcome,
}

impl<'known> Sorter<'known> {
  fn new(known: &'known KnownAccounts) -> Self {
    Self {
      known,
      seen: HashSet::new(),
      outcome: ImportOutcome::default(),
    }
  }

  fn place(&mut self, label: String, candidate: Result<ParsedAccount, SkipReason>) {
    let account = match candidate {
      Ok(account) => account,
      Err(reason) => {
        self.outcome.skipped.push(SkippedEntry { label, reason });
        return;
      }
    };

    let key = AccountKey::from_account(&account);
    let duplicate = if self.known.0.contains(&key) {
      Some(SkipReason::AlreadyInVault)
    } else if self.seen.insert(key) {
      None
    } else {
      Some(SkipReason::DuplicateInFile)
    };

    match duplicate {
      Some(reason) => self.outcome.duplicates.push(SkippedEntry { label, reason }),
      None => self.outcome.imported.push(account),
    }
  }
}

#[cfg(test)]
mod tests {
  use otp::Algorithm;
  use qr::OtpParams;
  use uuid::Uuid;

  use super::*;

  fn account(issuer: Option<&str>, account_label: &str, seed: &[u8]) -> ParsedAccount {
    ParsedAccount::new(
      issuer,
      account_label,
      seed.to_vec().into(),
      Algorithm::Sha1,
      6,
      OtpParams::Totp { period: 30 },
    )
    .unwrap()
  }

  fn entry(account: ParsedAccount, deleted_at: Option<u64>) -> Entry {
    let (issuer, account_label, otp) = account.into_parts();
    Entry {
      id: Uuid::now_v7(),
      issuer,
      account_label,
      otp,
      icon: None,
      tags: Vec::new(),
      notes: None,
      created_at: 0,
      updated_at: 0,
      deleted_at,
    }
  }

  fn labels(entries: &[SkippedEntry]) -> Vec<(&str, String)> {
    entries
      .iter()
      .map(|skipped| (skipped.label.as_str(), skipped.reason.to_string()))
      .collect()
  }

  #[test]
  fn place_sorts_new_known_repeated_and_invalid_entries() {
    let vault: Vec<Entry> = vec![entry(account(Some("GitHub"), "octocat", b"seed-1"), None)];
    let known: KnownAccounts = vault.iter().collect();
    let mut sorter = Sorter::new(&known);

    // Same account in another letter case and spacing: already known.
    sorter.place(
      "a".into(),
      Ok(account(Some(" github "), "OctoCat", b"seed-1")),
    );
    // Same names, different secret: a different enrolment.
    sorter.place(
      "b".into(),
      Ok(account(Some("GitHub"), "octocat", b"seed-2")),
    );
    // The same account again in the file.
    sorter.place(
      "c".into(),
      Ok(account(Some("GitHub"), "octocat", b"seed-2")),
    );
    sorter.place("d".into(), Err(QrError::InvalidSecret.into()));

    let outcome = sorter.outcome;
    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(
      labels(&outcome.duplicates),
      [
        ("a", "already in the vault".to_string()),
        ("c", "appears more than once in the file".to_string()),
      ]
    );
    assert_eq!(
      labels(&outcome.skipped),
      [("d", "the secret is not valid base32".to_string())]
    );
  }

  #[test]
  fn the_otp_scheme_is_part_of_the_identity() {
    let totp = account(None, "alice", b"seed");
    let hotp = ParsedAccount::new(
      None,
      "alice",
      b"seed".to_vec().into(),
      Algorithm::Sha1,
      6,
      OtpParams::Hotp { counter: 0 },
    )
    .unwrap();
    let known: KnownAccounts = [entry(totp, None)].iter().collect();
    let mut sorter = Sorter::new(&known);

    sorter.place("hotp".into(), Ok(hotp));

    assert_eq!(sorter.outcome.imported.len(), 1);
  }

  #[test]
  fn deleted_entries_are_not_known_when_collected_from_active_entries() {
    let mut payload = vault_core::VaultPayload::new();
    payload
      .entries
      .push(entry(account(None, "alice", b"seed"), Some(1)));
    let known: KnownAccounts = payload.active_entries().collect();
    let mut sorter = Sorter::new(&known);

    sorter.place("alice".into(), Ok(account(None, "alice", b"seed")));

    assert_eq!(sorter.outcome.imported.len(), 1);
  }

  #[test]
  fn display_label_uses_whichever_parts_exist() {
    assert_eq!(
      display_label(Some(" GitHub "), Some("octocat")).as_deref(),
      Some("GitHub (octocat)")
    );
    assert_eq!(
      display_label(Some(""), Some("octocat")).as_deref(),
      Some("octocat")
    );
    assert_eq!(
      display_label(Some("GitHub"), None).as_deref(),
      Some("GitHub")
    );
    assert_eq!(display_label(Some(" "), Some("")), None);
  }

  #[test]
  fn an_unparseable_entry_is_named_by_its_name_or_position() {
    let raw: Vec<&RawValue> =
      serde_json::from_str(r#"[{"name": "Example", "digits": 300}, {"digits": 300}]"#).unwrap();
    assert_eq!(fallback_label(raw[0], 0), "Example");
    assert_eq!(fallback_label(raw[1], 1), "entry 2");
  }
}
