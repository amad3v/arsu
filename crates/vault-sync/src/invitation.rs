//! Invitations: how a new device is introduced.
//!
//! A device already in the vault's device set opens an invitation: a
//! fresh 256-bit pairing secret, valid for [`LIFETIME`] seconds and for
//! one pairing. It is shown two ways:
//!
//! * a **QR code** of [`Invitation::qr_text`]: the secret and the
//!   inviting device's key, for a phone to photograph;
//! * a **text code**, [`Invitation::code`]: the secret alone, with a
//!   checksum, for a device that cannot read the QR code to type, and
//!   read back by [`parse_code`].
//!
//! Both use Crockford's base32, which is case-insensitive, reads `O` as
//! zero and `I`/`L` as one, and is uppercase-alphanumeric, so the QR
//! code can use its compact alphanumeric mode.

use sha2::{Digest, Sha256};
use vault_core::{PUBLIC_KEY_LEN, PendingInvitation, PublicKey, SecretBytes, SyncState};
use zeroize::Zeroizing;

use crate::noise::{PairingSecret, SECRET_LEN};

/// How long an invitation can be used: 15 minutes.
pub const LIFETIME: u64 = 15 * 60;

/// Most invitations open at once; opening another drops the oldest.
pub const MAX_OPEN: usize = 8;

/// Length of the advertisement token.
pub const TOKEN_LEN: usize = 8;

/// What the QR code's text starts with.
const QR_PREFIX: &str = "ARSU-INVITE:";

/// The QR payload format version.
const QR_VERSION: u8 = 1;

/// Length of the text code's checksum.
const CHECKSUM_LEN: usize = 2;

/// Characters per group in the text code.
const GROUP_LEN: usize = 5;

/// Length of the QR payload: version, secret, key.
const QR_LEN: usize = 1 + SECRET_LEN + PUBLIC_KEY_LEN;

/// Length of the text code's payload: secret and checksum.
const CODE_BYTES: usize = SECRET_LEN + CHECKSUM_LEN;

/// Characters in the text code without separators: five bits each.
const CODE_CHARS: usize = (CODE_BYTES * 8).div_ceil(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvitationError {
  #[error("this is not an Arsu invitation")]
  NotAnInvitation,
  #[error("this invitation was made by a newer Arsu: update this one")]
  UnsupportedVersion,
  #[error("the code is mistyped: check it and try again")]
  BadChecksum,
  #[error("set up sync on this device first")]
  NoIdentity,
  #[error("could not generate a pairing secret")]
  Random,
}

/// An invitation as the inviting device shows it, and as the joining
/// device reads it from the QR code. (A typed code carries the secret
/// alone: see [`parse_code`].)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
  pub secret: PairingSecret,
  /// The inviting device.
  pub inviter: PublicKey,
}

impl Invitation {
  /// The text of the QR code: `ARSU-INVITE:` and the base32 of the
  /// version, the secret and the inviting device's key.
  #[must_use]
  pub fn qr_text(&self) -> Zeroizing<String> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(QR_LEN));
    bytes.push(QR_VERSION);
    bytes.extend_from_slice(self.secret.expose());
    bytes.extend_from_slice(&self.inviter.0);
    let encoded = Zeroizing::new(base32::encode(base32::Alphabet::Crockford, &bytes));
    Zeroizing::new(format!("{QR_PREFIX}{}", encoded.as_str()))
  }

  /// Reads the text of a QR code.
  ///
  /// # Errors
  ///
  /// [`InvitationError::NotAnInvitation`] for any other text,
  /// [`InvitationError::UnsupportedVersion`] for a newer format.
  pub fn from_qr_text(text: &str) -> Result<Self, InvitationError> {
    let encoded = text
      .strip_prefix(QR_PREFIX)
      .ok_or(InvitationError::NotAnInvitation)?;
    let bytes = decode(encoded)?;
    match bytes.first() {
      Some(&QR_VERSION) if bytes.len() == QR_LEN => {}
      Some(&version) if version > QR_VERSION => return Err(InvitationError::UnsupportedVersion),
      _ => return Err(InvitationError::NotAnInvitation),
    }
    let (secret, inviter) = bytes[1..].split_at(SECRET_LEN);
    Ok(Self {
      secret: secret_from(secret)?,
      inviter: PublicKey(
        inviter
          .try_into()
          .map_err(|_| InvitationError::NotAnInvitation)?,
      ),
    })
  }

  /// The text code: the secret and a checksum in base32, in groups of
  /// five (`XXXXX-XXXXX-…`, 11 groups). Read it back with [`parse_code`].
  #[must_use]
  pub fn code(&self) -> Zeroizing<String> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(CODE_BYTES));
    bytes.extend_from_slice(self.secret.expose());
    bytes.extend_from_slice(&checksum(&self.secret));
    let encoded = Zeroizing::new(base32::encode(base32::Alphabet::Crockford, &bytes));
    let groups: Vec<&str> = encoded
      .as_bytes()
      .chunks(GROUP_LEN)
      // Crockford's alphabet is ASCII, so every chunk is valid UTF-8.
      .filter_map(|group| std::str::from_utf8(group).ok())
      .collect();
    Zeroizing::new(groups.join("-"))
  }
}

/// Reads a typed invitation code, ignoring case, spaces and dashes: the
/// pairing secret it carries.
///
/// # Errors
///
/// [`InvitationError::NotAnInvitation`] if it is not a code at all,
/// [`InvitationError::BadChecksum`] if it was mistyped.
pub fn parse_code(code: &str) -> Result<PairingSecret, InvitationError> {
  let compact: Zeroizing<String> = Zeroizing::new(
    code
      .chars()
      .filter(|c| !c.is_whitespace() && *c != '-')
      .collect(),
  );
  if compact.len() != CODE_CHARS {
    return Err(InvitationError::NotAnInvitation);
  }
  let bytes = decode(&compact)?;
  let (secret, sum) = bytes.split_at(SECRET_LEN);
  let secret = secret_from(secret)?;
  if sum != checksum(&secret) {
    return Err(InvitationError::BadChecksum);
  }
  Ok(secret)
}

/// Reads an invitation in either form, as the joining device gets it:
/// the text of its QR code (the secret and the inviting device's key) or
/// a typed code (the secret alone).
///
/// # Errors
///
/// As [`Invitation::from_qr_text`] or [`parse_code`].
pub fn read(text: &str) -> Result<(PairingSecret, Option<PublicKey>), InvitationError> {
  let text = text.trim();
  if text.starts_with(QR_PREFIX) {
    let invitation = Invitation::from_qr_text(text)?;
    Ok((invitation.secret, Some(invitation.inviter)))
  } else {
    Ok((parse_code(text)?, None))
  }
}

/// What the inviting device advertises in pairing mode, so the joining
/// device connects to the right one: the first bytes of
/// SHA-256 over a label and the secret. It reveals nothing about the
/// secret.
#[must_use]
pub fn advertisement_token(secret: &PairingSecret) -> [u8; TOKEN_LEN] {
  let digest = labelled_hash(b"arsu-sync advertisement", secret);
  let mut token = [0u8; TOKEN_LEN];
  token.copy_from_slice(&digest[..TOKEN_LEN]);
  token
}

/// Opens a new invitation from this device, valid for [`LIFETIME`].
/// Expired invitations are dropped, and the oldest beyond [`MAX_OPEN`].
///
/// # Errors
///
/// [`InvitationError::NoIdentity`] if sync is not set up here;
/// [`InvitationError::Random`] if no secret can be generated.
pub fn open(state: &mut SyncState, now: u64) -> Result<Invitation, InvitationError> {
  let inviter = state
    .identity
    .as_ref()
    .ok_or(InvitationError::NoIdentity)?
    .public_key;
  let secret = PairingSecret::generate().map_err(|_| InvitationError::Random)?;
  state
    .invitations
    .retain(|invitation| invitation.expires_at >= now);
  state.invitations.push(PendingInvitation {
    secret: secret.expose().to_vec().into(),
    expires_at: now.saturating_add(LIFETIME),
  });
  let excess = state.invitations.len().saturating_sub(MAX_OPEN);
  state.invitations.drain(..excess);
  Ok(Invitation { secret, inviter })
}

/// The secrets of the invitations still valid at `now`.
#[must_use]
pub fn open_secrets(state: &SyncState, now: u64) -> Vec<PairingSecret> {
  state
    .invitations
    .iter()
    .filter(|invitation| invitation.expires_at >= now)
    .filter_map(|invitation| secret_from(invitation.secret.expose_secret()).ok())
    .collect()
}

/// Uses up the invitation with this secret. Returns whether it was open.
pub fn consume(state: &mut SyncState, secret: &PairingSecret) -> bool {
  let secret: SecretBytes = secret.expose().to_vec().into();
  let before = state.invitations.len();
  state
    .invitations
    .retain(|invitation| invitation.secret != secret);
  state.invitations.len() < before
}

/// Decodes Crockford base32 into a zeroizing buffer.
fn decode(encoded: &str) -> Result<Zeroizing<Vec<u8>>, InvitationError> {
  base32::decode(base32::Alphabet::Crockford, encoded)
    .map(Zeroizing::new)
    .ok_or(InvitationError::NotAnInvitation)
}

fn secret_from(bytes: &[u8]) -> Result<PairingSecret, InvitationError> {
  let bytes: [u8; SECRET_LEN] = bytes
    .try_into()
    .map_err(|_| InvitationError::NotAnInvitation)?;
  Ok(PairingSecret::from_bytes(bytes))
}

/// The text code's checksum: catches typos, not tampering.
fn checksum(secret: &PairingSecret) -> [u8; CHECKSUM_LEN] {
  let digest = labelled_hash(b"arsu-sync code checksum", secret);
  [digest[0], digest[1]]
}

/// SHA-256 over a label, which keeps each use of the secret apart, and
/// the secret.
fn labelled_hash(label: &[u8], secret: &PairingSecret) -> [u8; 32] {
  let mut hasher = Sha256::new();
  hasher.update(label);
  hasher.update(secret.expose());
  hasher.finalize().into()
}

#[cfg(test)]
mod tests;
