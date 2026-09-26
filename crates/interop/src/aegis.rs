//! Import and export of Aegis vault files.
//!
//! Aegis publishes a full format specification
//! (<https://github.com/beemdevelopment/Aegis/blob/master/docs/vault.md>),
//! and every field name and the whole crypto scheme
//! below come from that document rather than from reverse engineering:
//!
//! - `db` is either the content as a JSON object (plain vault) or a
//!   base64 string (encrypted vault, AES-256-GCM under a 256-bit master
//!   key).
//! - The master key is stored once per credential, in `slots`, each
//!   copy wrapped (AES-256-GCM) under a key derived from that
//!   credential. Only password slots (`type: 1`) are usable here; the
//!   biometric ones are bound to an Android Keystore. A vault can hold
//!   several password slots (e.g. a separate backup password), and each
//!   is tried in turn.
//! - A password slot's key is scrypt(password, salt, N, r, p), with N,
//!   r and p read from the slot, so they are checked against explicit
//!   bounds before anything is derived: at most 1 GiB of memory and
//!   2 GiB of work per slot, and at most 8 slots.
//!
//! Aegis stores the GCM tag in its own hex field instead of appended to
//! the ciphertext, so the AES-GCM calls here use detached tags.

use std::ops::RangeInclusive;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use scrypt::{Params, scrypt};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use uuid::Uuid;
use zeroize::Zeroizing;

use qr::{
  DEFAULT_ALGORITHM, DEFAULT_DIGITS, OtpKind, OtpParams, ParsedAccount, QrError, algorithm_name,
  decode_secret, encode_secret, parse_algorithm,
};
use vault_core::{Entry, OtpConfig};

use crate::{
  ImportOutcome, InteropError, KnownAccounts, check_size,
  outcome::{SourceEntry, display_label, import_entries},
  secret::{
    KEY_LEN, Key, NONCE_LEN, SecretText, TAG_LEN, open, random_bytes, seal, to_json_zeroizing,
  },
};

/// The vault file format version (the outer JSON) this module reads and
/// writes.
const FILE_VERSION: u32 = 1;

/// The content (`db`) versions this module reads: every version Aegis
/// has written so far. Entries have the same shape in all of them;
/// like Aegis itself, a newer version is refused rather than guessed at.
const CONTENT_VERSIONS: RangeInclusive<u32> = 1..=3;

/// The content version exports are written in.
const EXPORT_CONTENT_VERSION: u32 = 3;

const PASSWORD_SLOT_TYPE: u8 = 1;

/// Aegis writes one slot per credential: a password, possibly a separate
/// backup password, and a biometric key. Anything beyond this is not a
/// real vault, and each password slot costs a key derivation to try.
const MAX_SLOTS: usize = 8;

/// The outer vault file, as read.
#[derive(Deserialize)]
struct VaultFile<'file> {
  version: u32,
  header: Header,
  /// The content as a JSON object, or the base64 of its encryption.
  #[serde(borrow)]
  db: &'file RawValue,
}

/// The outer vault file, as [`export`] writes it.
#[derive(Serialize)]
struct ExportFile {
  version: u32,
  header: Header,
  db: String,
}

#[derive(Serialize, Deserialize)]
struct Header {
  /// `None` for a plain vault.
  slots: Option<Vec<Slot>>,
  /// The content's nonce and tag; `None` for a plain vault.
  params: Option<EncParams>,
}

/// An AES-256-GCM nonce and tag, hex-encoded.
#[derive(Serialize, Deserialize)]
struct EncParams {
  nonce: String,
  tag: String,
}

#[derive(Serialize, Deserialize)]
struct Slot {
  #[serde(rename = "type")]
  kind: u8,
  #[serde(default)]
  uuid: String,
  /// The wrapped master key, hex-encoded.
  key: String,
  key_params: EncParams,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  n: Option<u32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  r: Option<u32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  p: Option<u32>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  salt: Option<String>,
}

/// The vault content, as read. Entries stay raw JSON here so that each
/// one is parsed on its own (see [`import_entries`]).
#[derive(Deserialize)]
struct VaultContent<'content> {
  version: u32,
  #[serde(borrow)]
  entries: Vec<&'content RawValue>,
}

/// The vault content, as [`export`] writes it.
#[derive(Serialize)]
struct ExportContent<'vault> {
  version: u32,
  entries: Vec<ExportEntry<'vault>>,
  /// Aegis's entry groups; none are exported.
  groups: Vec<serde_json::Value>,
}

/// One content entry, as read.
#[derive(Deserialize)]
struct RawEntry {
  #[serde(rename = "type")]
  kind: String,
  #[serde(default)]
  name: String,
  #[serde(default)]
  issuer: String,
  info: RawInfo,
}

#[derive(Deserialize)]
struct RawInfo {
  secret: SecretText,
  algo: Option<String>,
  digits: Option<u8>,
  period: Option<u64>,
  counter: Option<u64>,
}

/// One content entry, as [`export`] writes it.
#[derive(Serialize)]
struct ExportEntry<'vault> {
  #[serde(rename = "type")]
  kind: &'static str,
  uuid: Uuid,
  name: &'vault str,
  issuer: &'vault str,
  #[serde(skip_serializing_if = "Option::is_none")]
  note: Option<&'vault str>,
  info: ExportInfo,
  /// The UUIDs of the Aegis groups the entry is in; none are exported.
  groups: Vec<Uuid>,
}

#[derive(Serialize)]
struct ExportInfo {
  secret: SecretText,
  algo: &'static str,
  digits: u8,
  #[serde(skip_serializing_if = "Option::is_none")]
  period: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  counter: Option<u64>,
}

/// Imports the accounts in an Aegis vault file, leaving out those
/// `known` already holds.
///
/// `password` is required for an encrypted vault and ignored for a plain
/// one. Entry types without a published specification (`steam`,
/// `motp`, `yandex`) are reported in [`ImportOutcome::skipped`], as is
/// any entry that is malformed or fails validation.
///
/// # Errors
///
/// - [`InteropError::FileTooLarge`] over [`crate::MAX_IMPORT_BYTES`];
/// - [`InteropError::UnrecognizedFormat`] if the file or its decrypted
///   content is not an Aegis vault, [`InteropError::UnsupportedVersion`]
///   if it is one of a version this module doesn't know;
/// - [`InteropError::PasswordRequired`] for an encrypted vault without
///   a password;
/// - [`InteropError::Malformed`], [`InteropError::TooManySlots`] or
///   [`InteropError::UnsupportedKdfParams`] if the encryption header is
///   unusable (checked before any key derivation);
/// - [`InteropError::UnsupportedCredential`] if the vault has no
///   password slot;
/// - [`InteropError::WrongPasswordOrCorrupted`] if no password slot
///   opens with `password`, or the content does not decrypt.
pub fn import(
  file_bytes: &[u8],
  password: Option<&str>,
  known: &KnownAccounts,
) -> Result<ImportOutcome, InteropError> {
  check_size(file_bytes)?;
  let file: VaultFile<'_> =
    serde_json::from_slice(file_bytes).map_err(InteropError::UnrecognizedFormat)?;
  if file.version != FILE_VERSION {
    return Err(InteropError::UnsupportedVersion {
      found: file.version,
    });
  }

  let Some(slots) = &file.header.slots else {
    return import_content(file.db.get().as_bytes(), known);
  };
  let password = password.ok_or(InteropError::PasswordRequired)?;
  let params = file
    .header
    .params
    .as_ref()
    .ok_or(InteropError::Malformed("content encryption parameters"))?;
  let mut content = SealedContent::parse(params, file.db)?;
  let password_slots = parse_password_slots(slots)?;

  let master_key = unwrap_master_key(&password_slots, password)?;
  import_content(content.decrypt(&master_key)?, known)
}

fn import_content(content: &[u8], known: &KnownAccounts) -> Result<ImportOutcome, InteropError> {
  let content: VaultContent<'_> =
    serde_json::from_slice(content).map_err(InteropError::UnrecognizedFormat)?;
  if !CONTENT_VERSIONS.contains(&content.version) {
    return Err(InteropError::UnsupportedVersion {
      found: content.version,
    });
  }
  Ok(import_entries::<RawEntry>(&content.entries, known))
}

/// Decodes and checks every password slot, so that trying them costs
/// nothing but the key derivations.
fn parse_password_slots(slots: &[Slot]) -> Result<Vec<PasswordSlot>, InteropError> {
  if slots.len() > MAX_SLOTS {
    return Err(InteropError::TooManySlots);
  }
  let password_slots = slots
    .iter()
    .filter(|slot| slot.kind == PASSWORD_SLOT_TYPE)
    .map(PasswordSlot::try_from)
    .collect::<Result<Vec<_>, _>>()?;
  if password_slots.is_empty() {
    return Err(InteropError::UnsupportedCredential);
  }
  Ok(password_slots)
}

/// Tries `password` on each slot in turn, the way Aegis does.
fn unwrap_master_key(slots: &[PasswordSlot], password: &str) -> Result<Key, InteropError> {
  for slot in slots {
    if let Some(master_key) = slot.unwrap_master_key(password)? {
      return Ok(master_key);
    }
  }
  Err(InteropError::WrongPasswordOrCorrupted)
}

/// Exports `entries` as a password-encrypted Aegis vault file.
///
/// Always encrypted: there is deliberately no plaintext export, so a file
/// holding every seed never sits on disk unprotected. The password slot
/// uses Aegis's own scrypt parameters.
///
/// # Errors
///
/// Returns [`InteropError::Randomness`] if no key or nonce can be
/// generated, and [`InteropError::Encrypt`] or
/// [`InteropError::Serialize`] if the vault cannot be sealed or
/// written.
pub fn export<'vault>(
  entries: impl IntoIterator<Item = &'vault Entry>,
  password: &str,
) -> Result<Vec<u8>, InteropError> {
  let content = ExportContent {
    version: EXPORT_CONTENT_VERSION,
    entries: entries.into_iter().map(ExportEntry::from).collect(),
    groups: Vec::new(),
  };
  let master_key = random_bytes()?;
  let slot = Slot::for_password(&master_key, password, ScryptCost::AEGIS)?;
  seal_vault(&content, &master_key, vec![slot])
}

/// Encrypts `content` under `master_key` and writes the vault file,
/// with `slots` as the ways to unlock it.
fn seal_vault(
  content: &ExportContent<'_>,
  master_key: &Key,
  slots: Vec<Slot>,
) -> Result<Vec<u8>, InteropError> {
  let mut db = to_json_zeroizing(content)?;
  let nonce = random_bytes()?;
  let tag = seal(master_key, &nonce, &mut db)?;

  let file = ExportFile {
    version: FILE_VERSION,
    header: Header {
      slots: Some(slots),
      params: Some(EncParams::new(&nonce, &tag)),
    },
    // `db` holds the ciphertext now.
    db: BASE64.encode(&*db),
  };
  serde_json::to_vec_pretty(&file).map_err(InteropError::Serialize)
}

impl EncParams {
  fn new(nonce: &[u8; NONCE_LEN], tag: &[u8; TAG_LEN]) -> Self {
    Self {
      nonce: hex::encode(nonce),
      tag: hex::encode(tag),
    }
  }
}

/// The encrypted content, decoded and checked up front.
struct SealedContent {
  nonce: [u8; NONCE_LEN],
  tag: [u8; TAG_LEN],
  /// The ciphertext, and the plaintext once opened.
  buffer: Zeroizing<Vec<u8>>,
}

impl SealedContent {
  fn parse(params: &EncParams, db: &RawValue) -> Result<Self, InteropError> {
    let malformed = || InteropError::Malformed("encrypted content");
    let db: String = serde_json::from_str(db.get()).map_err(|_| malformed())?;
    Ok(Self {
      nonce: hex_array(&params.nonce, "content nonce")?,
      tag: hex_array(&params.tag, "content tag")?,
      buffer: Zeroizing::new(BASE64.decode(db).map_err(|_| malformed())?),
    })
  }

  /// Decrypts the content in place and returns the plaintext.
  fn decrypt(&mut self, master_key: &Key) -> Result<&[u8], InteropError> {
    open(master_key, &self.nonce, &mut self.buffer, &self.tag)
      .map_err(|_| InteropError::WrongPasswordOrCorrupted)?;
    Ok(&self.buffer)
  }
}

impl Slot {
  /// A new password slot for `password`, wrapping `master_key`.
  fn for_password(
    master_key: &Key,
    password: &str,
    cost: ScryptCost,
  ) -> Result<Self, InteropError> {
    let salt = random_bytes::<32>()?;
    let slot_key = cost.derive_key(password, salt.as_slice())?;
    let nonce = random_bytes()?;
    let mut wrapped_key = master_key.clone();
    let tag = seal(&slot_key, &nonce, wrapped_key.as_mut_slice())?;

    Ok(Self {
      kind: PASSWORD_SLOT_TYPE,
      uuid: Uuid::now_v7().to_string(),
      key: hex::encode(*wrapped_key),
      key_params: EncParams::new(&nonce, &tag),
      n: Some(cost.n()),
      r: Some(cost.r),
      p: Some(cost.p),
      salt: Some(hex::encode(salt)),
    })
  }
}

/// A password slot with every field decoded and checked.
struct PasswordSlot {
  cost: ScryptCost,
  salt: Vec<u8>,
  wrapped_key: [u8; KEY_LEN],
  nonce: [u8; NONCE_LEN],
  tag: [u8; TAG_LEN],
}

impl TryFrom<&Slot> for PasswordSlot {
  type Error = InteropError;

  fn try_from(slot: &Slot) -> Result<Self, InteropError> {
    let (Some(n), Some(r), Some(p)) = (slot.n, slot.r, slot.p) else {
      return Err(InteropError::Malformed("password slot's scrypt parameters"));
    };
    let salt = slot
      .salt
      .as_deref()
      .and_then(|salt| hex::decode(salt).ok())
      .ok_or(InteropError::Malformed("password slot's salt"))?;

    Ok(Self {
      cost: ScryptCost::new(n, r, p)?,
      salt,
      wrapped_key: hex_array(&slot.key, "password slot's key")?,
      nonce: hex_array(&slot.key_params.nonce, "password slot's nonce")?,
      tag: hex_array(&slot.key_params.tag, "password slot's tag")?,
    })
  }
}

impl PasswordSlot {
  /// Unwraps the master key with the key `password` derives, or returns
  /// `None` if `password` is not this slot's.
  fn unwrap_master_key(&self, password: &str) -> Result<Option<Key>, InteropError> {
    let slot_key = self.cost.derive_key(password, &self.salt)?;
    let mut master_key = Zeroizing::new(self.wrapped_key);
    Ok(
      open(&slot_key, &self.nonce, master_key.as_mut_slice(), &self.tag)
        .is_ok()
        .then_some(master_key),
    )
  }
}

/// scrypt cost parameters that passed the bounds check.
///
/// Aegis always uses N = 2^15, r = 8, p = 1 (32 MiB, a fraction of a
/// second). The bounds leave ample room above that, but cap what a
/// crafted file can demand: memory (128·r·N bytes) at 1 GiB and total
/// work (128·r·N·p bytes mixed) at 2 GiB, a few seconds per slot; with
/// [`MAX_SLOTS`], unlocking a hostile file is bounded too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScryptCost {
  log_n: u8,
  r: u32,
  p: u32,
}

impl ScryptCost {
  /// Aegis's own parameters, used for exports.
  const AEGIS: Self = Self {
    log_n: 15,
    r: 8,
    p: 1,
  };
  const LOG_N: RangeInclusive<u8> = 1..=20;
  const R: RangeInclusive<u32> = 1..=32;
  const P: RangeInclusive<u32> = 1..=16;
  const MAX_MEMORY_BYTES: u64 = 1 << 30;
  const MAX_WORK_BYTES: u64 = 2 << 30;

  fn new(n: u32, r: u32, p: u32) -> Result<Self, InteropError> {
    let unsupported = || InteropError::UnsupportedKdfParams { n, r, p };
    if !n.is_power_of_two() {
      return Err(unsupported());
    }
    let cost = Self {
      log_n: u8::try_from(n.ilog2()).map_err(|_| unsupported())?,
      r,
      p,
    };

    // Range checks first: they keep the byte counts below from
    // overflowing.
    let within_bounds = Self::LOG_N.contains(&cost.log_n)
      && Self::R.contains(&r)
      && Self::P.contains(&p)
      && cost.memory_bytes() <= Self::MAX_MEMORY_BYTES
      && cost.memory_bytes() * u64::from(p) <= Self::MAX_WORK_BYTES;
    within_bounds.then_some(cost).ok_or_else(unsupported)
  }

  const fn n(self) -> u32 {
    1 << self.log_n
  }

  /// The size of scrypt's working array, 128·r·N bytes.
  fn memory_bytes(self) -> u64 {
    (128 * u64::from(self.r)) << self.log_n
  }

  fn derive_key(self, password: &str, salt: &[u8]) -> Result<Key, InteropError> {
    let params =
      Params::new(self.log_n, self.r, self.p).map_err(|_| InteropError::UnsupportedKdfParams {
        n: self.n(),
        r: self.r,
        p: self.p,
      })?;
    let mut key = Zeroizing::new([0; KEY_LEN]);
    scrypt(password.as_bytes(), salt, &params, key.as_mut_slice())
      .expect("a 32-byte output is within scrypt's output length limits");
    Ok(key)
  }
}

fn hex_array<const N: usize>(hex: &str, field: &'static str) -> Result<[u8; N], InteropError> {
  let mut bytes = [0; N];
  hex::decode_to_slice(hex, &mut bytes).map_err(|_| InteropError::Malformed(field))?;
  Ok(bytes)
}

impl SourceEntry for RawEntry {
  fn label(&self) -> Option<String> {
    display_label(Some(&self.issuer), Some(&self.name))
  }

  fn into_account(self) -> Result<ParsedAccount, QrError> {
    let kind: OtpKind = self.kind.parse()?;
    let info = self.info;
    let params = OtpParams::for_kind(kind, info.period, info.counter)?;
    let algorithm = info
      .algo
      .as_deref()
      .map_or(Ok(DEFAULT_ALGORITHM), parse_algorithm)?;

    ParsedAccount::new(
      Some(&self.issuer),
      &self.name,
      decode_secret(info.secret.expose_secret())?,
      algorithm,
      info.digits.unwrap_or(DEFAULT_DIGITS),
      params,
    )
  }
}

impl<'vault> From<&'vault Entry> for ExportEntry<'vault> {
  fn from(entry: &'vault Entry) -> Self {
    let (kind, secret, algorithm, digits, period, counter) = match &entry.otp {
      OtpConfig::Totp {
        secret,
        algorithm,
        digits,
        period,
      } => ("totp", secret, algorithm, digits, Some(*period), None),
      OtpConfig::Hotp {
        secret,
        algorithm,
        digits,
        counter,
      } => ("hotp", secret, algorithm, digits, None, Some(*counter)),
    };

    Self {
      kind,
      uuid: entry.id,
      name: &entry.account_label,
      issuer: entry.issuer.as_deref().unwrap_or_default(),
      note: entry.notes.as_deref(),
      info: ExportInfo {
        secret: encode_secret(secret).into(),
        algo: algorithm_name(*algorithm),
        digits: *digits,
        period,
        counter,
      },
      groups: Vec::new(),
    }
  }
}

#[cfg(test)]
mod tests {
  use otp::{Algorithm, OtpError};
  use serde_json::{Value, json};

  use crate::SkipReason;

  use super::*;

  /// Imports into an empty vault.
  fn import_fresh(
    file_bytes: &[u8],
    password: Option<&str>,
  ) -> Result<ImportOutcome, InteropError> {
    import(file_bytes, password, &KnownAccounts::default())
  }

  /// Cheap enough for tests, yet within the bounds.
  const TEST_COST: ScryptCost = ScryptCost {
    log_n: 10,
    r: 8,
    p: 1,
  };

  /// The `header` block of Aegis's own test fixture
  /// (beemdevelopment/Aegis, `app/src/test/resources/.../importers/
  /// aegis_encrypted.json`): real values. The `db` is a placeholder,
  /// since the fixture's password is not published.
  const REAL_HEADER_JSON: &str = r#"{
        "version": 1,
        "header": {
            "slots": [
                {
                    "type": 1,
                    "uuid": "a8325752-c1be-458a-9b3e-5e0a8154d9ec",
                    "key": "491d44550430ba248986b904b8cffd3a6c5755d176ac877bd11b82c934225017",
                    "key_params": {
                        "nonce": "e9705513ba4951fa7a0608d2",
                        "tag": "931237af257b83c693ddb8f9a7eddaf0"
                    },
                    "n": 32768,
                    "r": 8,
                    "p": 1,
                    "salt": "27ea9ae53fa2f08a8dcd201615a8229422647b3058f9f36b08f9457e62888be1",
                    "repaired": true
                }
            ],
            "params": {
                "nonce": "095fd13dee336fa56b4634ff",
                "tag": "5db2470edf2d12f82a89ae7f48ccd50c"
            }
        },
        "db": "cGxhY2Vob2xkZXI="
    }"#;

  fn entry(issuer: Option<&str>, account_label: &str, otp: OtpConfig) -> Entry {
    Entry {
      id: Uuid::now_v7(),
      issuer: issuer.map(str::to_owned),
      account_label: account_label.to_owned(),
      otp,
      icon: None,
      tags: Vec::new(),
      notes: Some("recovery codes in the safe".to_owned()),
      created_at: 0,
      updated_at: 0,
      deleted_at: None,
    }
  }

  fn totp_entry() -> Entry {
    entry(
      Some("Example Co"),
      "alice@example.com",
      OtpConfig::Totp {
        secret: b"12345678901234567890".to_vec().into(),
        algorithm: Algorithm::Sha1,
        digits: 6,
        period: 30,
      },
    )
  }

  fn assert_imports_as(account: &ParsedAccount, entry: &Entry) {
    assert_eq!(account.issuer(), entry.issuer.as_deref());
    assert_eq!(account.account_label(), entry.account_label);
    assert_eq!(account.otp(), &entry.otp);
  }

  fn import_plain(entries: &Value) -> ImportOutcome {
    let file = json!({
      "version": 1,
      "header": { "slots": null, "params": null },
      "db": { "version": 3, "entries": entries, "groups": [] },
    });
    import_fresh(&serde_json::to_vec(&file).unwrap(), None).unwrap()
  }

  /// A vault whose password slots have the given scrypt parameters;
  /// every other field is well-formed.
  fn vault_with_slot_costs(costs: &[(u32, u32, u32)]) -> Vec<u8> {
    let slots: Vec<Value> = costs
      .iter()
      .map(|&(n, r, p)| {
        json!({
          "type": 1,
          "key": "00".repeat(KEY_LEN),
          "key_params": { "nonce": "00".repeat(NONCE_LEN), "tag": "00".repeat(TAG_LEN) },
          "n": n, "r": r, "p": p,
          "salt": "00".repeat(32),
        })
      })
      .collect();
    let params = json!({ "nonce": "00".repeat(NONCE_LEN), "tag": "00".repeat(TAG_LEN) });
    serde_json::to_vec(&json!({
      "version": 1,
      "header": { "slots": slots, "params": params },
      "db": "AAAA",
    }))
    .unwrap()
  }

  /// An encrypted vault holding `entries`, with the given slots wrapping
  /// its master key.
  fn sealed_vault(entries: &[Entry], slots: impl FnOnce(&Key) -> Vec<Slot>) -> Vec<u8> {
    let content = ExportContent {
      version: EXPORT_CONTENT_VERSION,
      entries: entries.iter().map(ExportEntry::from).collect(),
      groups: Vec::new(),
    };
    let master_key = random_bytes().unwrap();
    seal_vault(&content, &master_key, slots(&master_key)).unwrap()
  }

  fn password_slot(master_key: &Key, password: &str) -> Slot {
    Slot::for_password(master_key, password, TEST_COST).unwrap()
  }

  fn biometric_slot() -> Slot {
    Slot {
      kind: 2,
      uuid: String::new(),
      key: "00".repeat(KEY_LEN),
      key_params: EncParams::new(&[0; NONCE_LEN], &[0; TAG_LEN]),
      n: None,
      r: None,
      p: None,
      salt: None,
    }
  }

  // ---- format ----------------------------------------------------------

  #[test]
  fn deserializes_the_real_aegis_fixture_header() {
    let file: VaultFile<'_> = serde_json::from_str(REAL_HEADER_JSON).unwrap();
    assert_eq!(file.version, FILE_VERSION);

    let slots = parse_password_slots(file.header.slots.as_deref().unwrap()).unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].cost, ScryptCost::AEGIS);
    assert_eq!(slots[0].salt.len(), 32);

    SealedContent::parse(file.header.params.as_ref().unwrap(), file.db).unwrap();
  }

  #[test]
  fn export_then_import_round_trips() {
    let entries = [
      totp_entry(),
      entry(
        None,
        "bob",
        OtpConfig::Hotp {
          secret: b"12345678901234567890".to_vec().into(),
          algorithm: Algorithm::Sha256,
          digits: 8,
          counter: 42,
        },
      ),
    ];
    let file_bytes = export(&entries, "correct horse battery staple").unwrap();

    let outcome = import_fresh(&file_bytes, Some("correct horse battery staple")).unwrap();
    assert_eq!(outcome.imported.len(), 2);
    assert!(outcome.skipped.is_empty() && outcome.duplicates.is_empty());
    assert_imports_as(&outcome.imported[0], &entries[0]);
    assert_imports_as(&outcome.imported[1], &entries[1]);
  }

  #[test]
  fn export_writes_aegis_parameters_and_entry_identity() {
    let entries = [totp_entry()];
    let file_bytes = export(&entries, "password").unwrap();

    let file: VaultFile<'_> = serde_json::from_slice(&file_bytes).unwrap();
    let slots = parse_password_slots(file.header.slots.as_deref().unwrap()).unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].cost, ScryptCost::AEGIS);

    let master_key = unwrap_master_key(&slots, "password").unwrap();
    let mut content = SealedContent::parse(file.header.params.as_ref().unwrap(), file.db).unwrap();
    let content: Value = serde_json::from_slice(content.decrypt(&master_key).unwrap()).unwrap();
    assert_eq!(content["version"], json!(EXPORT_CONTENT_VERSION));

    let exported = &content["entries"][0];
    assert_eq!(exported["uuid"], json!(entries[0].id));
    assert_eq!(exported["note"], json!("recovery codes in the safe"));
    assert_eq!(exported["info"]["algo"], json!("SHA1"));
    assert_eq!(
      exported["info"]["secret"],
      json!("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ")
    );
  }

  #[test]
  fn import_with_wrong_password_is_rejected() {
    let file_bytes = export(&[totp_entry()], "correct password").unwrap();
    assert!(matches!(
      import_fresh(&file_bytes, Some("wrong password")),
      Err(InteropError::WrongPasswordOrCorrupted)
    ));
  }

  #[test]
  fn import_encrypted_vault_without_password_is_an_error() {
    let file_bytes = export(&[totp_entry()], "a password").unwrap();
    assert!(matches!(
      import_fresh(&file_bytes, None),
      Err(InteropError::PasswordRequired)
    ));
  }

  #[test]
  fn unknown_versions_are_refused() {
    let mut file: Value = serde_json::from_str(REAL_HEADER_JSON).unwrap();
    file["version"] = json!(2);
    assert!(matches!(
      import_fresh(&serde_json::to_vec(&file).unwrap(), Some("pw")),
      Err(InteropError::UnsupportedVersion { found: 2 })
    ));

    let file = json!({
      "version": 1,
      "header": { "slots": null, "params": null },
      "db": { "version": 4, "entries": [] },
    });
    assert!(matches!(
      import_fresh(&serde_json::to_vec(&file).unwrap(), None),
      Err(InteropError::UnsupportedVersion { found: 4 })
    ));
  }

  // ---- entries ---------------------------------------------------------

  #[test]
  fn unsupported_entry_type_is_skipped_not_fatal() {
    let outcome = import_plain(&json!([
      {
        "type": "steam", "name": "Steam Guard", "issuer": "Steam",
        "info": { "secret": "JBSWY3DPEHPK3PXP", "algo": "SHA1", "digits": 5 }
      },
      {
        "type": "totp", "name": "alice@example.com", "issuer": "Example",
        "info": { "secret": "JBSWY3DPEHPK3PXP", "algo": "SHA1", "digits": 6, "period": 30 }
      }
    ]));

    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].label, "Steam (Steam Guard)");
    assert!(matches!(
      &outcome.skipped[0].reason,
      SkipReason::Invalid(QrError::UnknownType(kind)) if kind == "steam"
    ));
  }

  #[test]
  fn a_malformed_entry_is_skipped_and_the_rest_import() {
    let outcome = import_plain(&json!([
      { "type": "totp", "name": "Too many digits", "issuer": "X", "info": { "secret": "JBSWY3DPEHPK3PXP", "digits": 300 } },
      { "type": "totp", "name": "no issuer field", "info": { "secret": "JBSWY3DPEHPK3PXP" } },
      { "type": "totp", "issuer": "No info" },
    ]));

    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.imported[0].issuer(), None);
    assert_eq!(outcome.imported[0].account_label(), "no issuer field");

    let skipped: Vec<&str> = outcome.skipped.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(skipped, ["Too many digits", "entry 3"]);
    assert!(
      outcome
        .skipped
        .iter()
        .all(|s| matches!(s.reason, SkipReason::Malformed(_)))
    );
  }

  #[test]
  fn algorithm_and_secret_follow_the_shared_rules() {
    let outcome = import_plain(&json!([{
      "type": "TOTP", "name": "alice", "issuer": "Example",
      "info": { "secret": "jbsw y3dp ehpk 3pxp", "algo": "sha256", "digits": 6, "period": 30 }
    }]));

    assert_eq!(outcome.imported.len(), 1, "{:?}", outcome.skipped);
    assert!(matches!(
      outcome.imported[0].otp(),
      OtpConfig::Totp {
        algorithm: Algorithm::Sha256,
        ..
      }
    ));
  }

  #[test]
  fn invalid_accounts_are_skipped_with_their_reason() {
    let outcome = import_plain(&json!([
      { "type": "hotp", "name": "exhausted", "info": { "secret": "JBSWY3DPEHPK3PXP", "counter": u64::MAX } },
      { "type": "hotp", "name": "no counter", "info": { "secret": "JBSWY3DPEHPK3PXP" } },
      { "type": "totp", "name": "user:1", "info": { "secret": "JBSWY3DPEHPK3PXP" } },
      { "type": "totp", "name": "", "issuer": "", "info": { "secret": "JBSWY3DPEHPK3PXP" } },
    ]));

    assert!(outcome.imported.is_empty());
    let reasons: Vec<&SkipReason> = outcome.skipped.iter().map(|s| &s.reason).collect();
    assert!(matches!(
      reasons[..],
      [
        SkipReason::Invalid(QrError::InvalidOtpConfig(OtpError::CounterExhausted)),
        SkipReason::Invalid(QrError::MissingCounter),
        SkipReason::Invalid(QrError::LabelContainsColon),
        SkipReason::Invalid(QrError::MissingLabel),
      ]
    ));
  }

  #[test]
  fn duplicates_of_the_vault_and_within_the_file_are_reported() {
    let existing = totp_entry();
    let other = entry(
      Some("Other"),
      "bob",
      OtpConfig::Totp {
        secret: b"another seed".to_vec().into(),
        algorithm: Algorithm::Sha1,
        digits: 6,
        period: 30,
      },
    );
    let known: KnownAccounts = [&existing].into_iter().collect();
    let file_bytes = export(&[existing, other.clone(), other], "pw").unwrap();

    let outcome = import(&file_bytes, Some("pw"), &known).unwrap();

    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.imported[0].account_label(), "bob");
    let duplicates: Vec<(&str, &SkipReason)> = outcome
      .duplicates
      .iter()
      .map(|s| (s.label.as_str(), &s.reason))
      .collect();
    assert!(matches!(
      duplicates[..],
      [
        ("Example Co (alice@example.com)", SkipReason::AlreadyInVault),
        ("Other (bob)", SkipReason::DuplicateInFile),
      ]
    ));
  }

  #[test]
  fn deeply_nested_entries_cannot_overflow_the_stack() {
    const DEPTH: usize = 100_000;
    let file = format!(
      r#"{{"version":1,"header":{{"slots":null,"params":null}},"db":{{"version":3,"entries":[
        {{"type":"totp","name":"deep","info":{{"secret":"JBSWY3DPEHPK3PXP"}},"icon":{open}{close}}},
        {{"type":"totp","name":"deeper","info":{open}{close}}}
      ]}}}}"#,
      open = "[".repeat(DEPTH),
      close = "]".repeat(DEPTH),
    );

    // Imports run on tokio's blocking pool, whose threads get 2 MiB.
    let outcome = std::thread::Builder::new()
      .stack_size(2 << 20)
      .spawn(move || import_fresh(file.as_bytes(), None))
      .unwrap()
      .join()
      .unwrap()
      .unwrap();

    assert_eq!(outcome.imported.len(), 1);
    assert_eq!(outcome.skipped[0].label, "deeper");
  }

  // ---- password slots ----------------------------------------------------

  #[test]
  fn every_password_slot_is_tried() {
    let entries = [totp_entry()];
    // A separate backup password first, then a biometric slot, then the
    // main password.
    let file_bytes = sealed_vault(&entries, |master_key| {
      vec![
        password_slot(master_key, "backup password"),
        biometric_slot(),
        password_slot(master_key, "main password"),
      ]
    });

    for password in ["main password", "backup password"] {
      let outcome = import_fresh(&file_bytes, Some(password)).unwrap();
      assert_imports_as(&outcome.imported[0], &entries[0]);
    }
    assert!(matches!(
      import_fresh(&file_bytes, Some("neither")),
      Err(InteropError::WrongPasswordOrCorrupted)
    ));
  }

  #[test]
  fn a_decoy_slot_does_not_hide_the_real_one() {
    let entries = [totp_entry()];
    let decoy_key = random_bytes().unwrap();
    let file_bytes = sealed_vault(&entries, |master_key| {
      vec![
        password_slot(&decoy_key, "other password"),
        password_slot(master_key, "password"),
      ]
    });

    let outcome = import_fresh(&file_bytes, Some("password")).unwrap();
    assert_imports_as(&outcome.imported[0], &entries[0]);
  }

  #[test]
  fn a_vault_without_password_slots_is_unsupported() {
    let file_bytes = sealed_vault(&[totp_entry()], |_| vec![biometric_slot()]);
    assert!(matches!(
      import_fresh(&file_bytes, Some("password")),
      Err(InteropError::UnsupportedCredential)
    ));
  }

  #[test]
  fn too_many_slots_are_refused() {
    let file_bytes = vault_with_slot_costs(&[(1 << 15, 8, 1); MAX_SLOTS + 1]);
    assert!(matches!(
      import_fresh(&file_bytes, Some("password")),
      Err(InteropError::TooManySlots)
    ));
  }

  #[test]
  fn malformed_slots_are_refused_before_any_key_derivation() {
    let mut file: Value =
      serde_json::from_slice(&vault_with_slot_costs(&[(1 << 15, 8, 1)])).unwrap();
    file["header"]["slots"][0]["key_params"]["nonce"] = json!("00");
    assert!(matches!(
      import_fresh(&serde_json::to_vec(&file).unwrap(), Some("pw")),
      Err(InteropError::Malformed("password slot's nonce"))
    ));
  }

  // ---- hostile scrypt parameters ------------------------------------------

  #[test]
  fn hostile_scrypt_parameters_are_refused_before_deriving() {
    for (n, r, p) in [
      // 2 TiB of memory.
      (1 << 31, 8, 1),
      // r·p overflows scrypt's own u32 arithmetic.
      (1 << 15, 65_536, 65_536),
      // ~128 GiB for scrypt's B array.
      (1 << 15, 1, (1 << 30) - 1),
      // An hour of CPU.
      (1 << 15, 8, 1 << 20),
      // N must be a power of two, and more than 1.
      (0, 8, 1),
      (1, 8, 1),
      (30_000, 8, 1),
      (1 << 15, 0, 1),
      (1 << 15, 8, 0),
    ] {
      let file_bytes = vault_with_slot_costs(&[(1 << 15, 8, 1), (n, r, p)]);
      let result = import_fresh(&file_bytes, Some("password"));
      assert!(
        matches!(
          result,
          Err(InteropError::UnsupportedKdfParams { n: got_n, r: got_r, p: got_p })
            if (got_n, got_r, got_p) == (n, r, p)
        ),
        "N={n}, r={r}, p={p}: {result:?}"
      );
    }
  }

  #[test]
  fn scrypt_bounds_admit_aegis_and_cap_memory_and_work() {
    assert_eq!(ScryptCost::new(1 << 15, 8, 1).unwrap(), ScryptCost::AEGIS);
    // Exactly 1 GiB of memory, or 2 GiB of work, is still allowed...
    assert!(ScryptCost::new(1 << 20, 8, 2).is_ok());
    assert!(ScryptCost::new(1 << 18, 32, 2).is_ok());
    // ...one step beyond either is not.
    assert!(ScryptCost::new(1 << 20, 16, 1).is_err());
    assert!(ScryptCost::new(1 << 20, 8, 3).is_err());
    assert!(ScryptCost::new(1 << 21, 1, 1).is_err());
    assert!(ScryptCost::new(1 << 15, 33, 1).is_err());
    assert!(ScryptCost::new(1 << 15, 8, 17).is_err());
  }
}
