//! Sync files and join files.
//!
//! A file is a one-way snapshot, encrypted to one recipient, for when
//! Bluetooth is not an option. Its layout:
//!
//! ```text
//! "ARSY" | version: u16 BE = 1 | kind: u8 (1 = sync, 2 = join)
//!        | frame: Noise handshake message (X or Xpsk1)
//!        | frame: Noise transport message           (repeated)
//! ```
//!
//! Frames and the chunks inside the transport messages are those of a
//! live session ([`crate::wire`]), carrying the same
//! [`Message::Snapshot`]. The kind selects the Noise pattern and its
//! prologue, so a file of one kind can never be read as the other.
//!
//! * A **sync file** (`Noise_X`) authenticates its sender, who must be
//!   trusted by the reader.
//! * A **join file** (`Noise_Xpsk1`) is bound to an invitation's secret
//!   instead: it introduces a device the reader did not know. The reader
//!   tries its open invitations and reports which one the file used, so
//!   the caller can consume it.

use snow::HandshakeState;
use vault_core::{DeviceIdentity, DeviceSet, PublicKey};
use zeroize::Zeroizing;

use crate::{
  devices,
  invitation::Invitation,
  noise::{self, PairingSecret, Purpose},
  protocol::{Message, ProtocolError, Snapshot},
  wire::{self, FrameReader, MAX_NOISE_MESSAGE, Reassembler, WireError},
};

/// What a file starts with.
pub const MAGIC: [u8; 4] = *b"ARSY";

/// The file format version this build writes and reads.
pub const FILE_VERSION: u16 = 1;

/// Longest file read: the longest message plus generous framing
/// overhead.
pub const MAX_FILE: usize = wire::MAX_MESSAGE + 64 * 1024;

/// The kind byte of a sync file.
const KIND_SYNC: u8 = 1;

/// The kind byte of a join file.
const KIND_JOIN: u8 = 2;

#[derive(Debug, thiserror::Error)]
pub enum FileError {
  #[error("this is not an Arsu sync file")]
  NotASyncFile,
  #[error("this sync file was made by a newer Arsu (format {0}): update this one")]
  UnsupportedVersion(u16),
  #[error("this sync file is for another device")]
  NotForThisDevice,
  #[error("this join file matches no open invitation: it may have expired or been used")]
  NoInvitation,
  #[error(
    "this sync file comes from a device ({}) that is not paired with your vault",
    .0.fingerprint()
  )]
  UntrustedSender(PublicKey),
  #[error("this sync file was made by this device")]
  OwnFile,
  #[error("this sync file is damaged or incomplete")]
  Damaged,
  #[error("could not encrypt the sync file")]
  Noise(#[from] snow::Error),
  #[error(transparent)]
  Wire(#[from] WireError),
  #[error(transparent)]
  Protocol(#[from] ProtocolError),
}

/// A file that was read and checked.
#[derive(Debug)]
pub struct ReceivedFile {
  /// The device that wrote it.
  pub sender: PublicKey,
  pub snapshot: Snapshot,
  /// For a join file, the invitation it used: consume it, and add the
  /// sender to the device set.
  pub invitation: Option<PairingSecret>,
}

/// A sync file of `snapshot` for `recipient`.
///
/// # Errors
///
/// [`FileError::OwnFile`] if `recipient` is this device;
/// [`FileError::Noise`], [`FileError::Wire`] or [`FileError::Protocol`]
/// if it cannot be built.
pub fn write_sync_file(
  identity: &DeviceIdentity,
  recipient: PublicKey,
  snapshot: Snapshot,
) -> Result<Vec<u8>, FileError> {
  write(Purpose::File, identity, None, recipient, snapshot)
}

/// A join file of `snapshot` for the device that showed `invitation`.
///
/// # Errors
///
/// As [`write_sync_file`].
pub fn write_join_file(
  identity: &DeviceIdentity,
  invitation: &Invitation,
  snapshot: Snapshot,
) -> Result<Vec<u8>, FileError> {
  write(
    Purpose::Join,
    identity,
    Some(&invitation.secret),
    invitation.inviter,
    snapshot,
  )
}

/// Reads a sync or join file meant for this device.
///
/// `trusted` is this device's device set (a sync file's sender must be
/// in it); `invitations` the secrets of its open invitations (a join
/// file must use one).
///
/// # Errors
///
/// Any [`FileError`].
pub fn read_file(
  bytes: &[u8],
  identity: &DeviceIdentity,
  trusted: &DeviceSet,
  invitations: &[PairingSecret],
) -> Result<ReceivedFile, FileError> {
  let (purpose, body) = parse_header(bytes)?;
  if bytes.len() > MAX_FILE {
    return Err(FileError::Damaged);
  }
  let mut reader = FrameReader::default();
  let frames = reader.push(body).map_err(|_| FileError::Damaged)?;
  if !reader.is_empty() {
    return Err(FileError::Damaged);
  }
  let (first, rest) = frames.split_first().ok_or(FileError::Damaged)?;

  let (handshake, invitation) = match purpose {
    Purpose::Join => {
      let found = invitations.iter().find_map(|secret| {
        open_handshake(Purpose::Join, identity, Some(secret), first)
          .ok()
          .map(|handshake| (handshake, secret.clone()))
      });
      let (handshake, secret) = found.ok_or(FileError::NoInvitation)?;
      (handshake, Some(secret))
    }
    _ => (
      open_handshake(Purpose::File, identity, None, first)
        .map_err(|_| FileError::NotForThisDevice)?,
      None,
    ),
  };

  let sender = handshake
    .get_remote_static()
    .and_then(noise::public_key)
    .ok_or(FileError::Damaged)?;
  if sender == identity.public_key {
    return Err(FileError::OwnFile);
  }
  if invitation.is_none() && !devices::is_trusted(trusted, &sender) {
    return Err(FileError::UntrustedSender(sender));
  }

  let message = decrypt_message(handshake, rest)?;
  match Message::decode(&message)? {
    Message::Snapshot(snapshot) => Ok(ReceivedFile {
      sender,
      snapshot: *snapshot,
      invitation,
    }),
    Message::Done => Err(FileError::Damaged),
  }
}

fn write(
  purpose: Purpose,
  identity: &DeviceIdentity,
  secret: Option<&PairingSecret>,
  recipient: PublicKey,
  snapshot: Snapshot,
) -> Result<Vec<u8>, FileError> {
  if recipient == identity.public_key {
    return Err(FileError::OwnFile);
  }
  let mut handshake = noise::handshake(purpose, true, identity, secret, Some(&recipient))?;
  let mut out = Vec::new();
  out.extend_from_slice(&MAGIC);
  out.extend_from_slice(&FILE_VERSION.to_be_bytes());
  out.push(if purpose == Purpose::Join {
    KIND_JOIN
  } else {
    KIND_SYNC
  });

  let mut ciphertext = vec![0u8; MAX_NOISE_MESSAGE];
  let length = handshake.write_message(&[], &mut ciphertext)?;
  out.extend(wire::frame(&ciphertext[..length])?);
  let mut transport = handshake.into_transport_mode()?;
  for chunk in wire::split(&Message::Snapshot(Box::new(snapshot)).encode()?)? {
    let length = transport.write_message(&chunk, &mut ciphertext)?;
    out.extend(wire::frame(&ciphertext[..length])?);
  }
  Ok(out)
}

/// The purpose a file's header names, and the frames after it.
fn parse_header(bytes: &[u8]) -> Result<(Purpose, &[u8]), FileError> {
  let rest = bytes.strip_prefix(&MAGIC).ok_or(FileError::NotASyncFile)?;
  let Some((&[high, low], rest)) = rest.split_first_chunk::<2>() else {
    return Err(FileError::NotASyncFile);
  };
  let version = u16::from_be_bytes([high, low]);
  if version != FILE_VERSION {
    return Err(FileError::UnsupportedVersion(version));
  }
  match rest.split_first() {
    Some((&KIND_SYNC, frames)) => Ok((Purpose::File, frames)),
    Some((&KIND_JOIN, frames)) => Ok((Purpose::Join, frames)),
    _ => Err(FileError::NotASyncFile),
  }
}

/// The recipient's handshake state after reading the file's handshake
/// message, if it was meant for this device (and secret).
fn open_handshake(
  purpose: Purpose,
  identity: &DeviceIdentity,
  secret: Option<&PairingSecret>,
  message: &[u8],
) -> Result<HandshakeState, snow::Error> {
  let mut handshake = noise::handshake(purpose, false, identity, secret, None)?;
  let mut payload = Zeroizing::new(vec![0u8; MAX_NOISE_MESSAGE]);
  handshake.read_message(message, &mut payload)?;
  Ok(handshake)
}

/// Decrypts the transport frames into the one message they must carry,
/// exactly.
fn decrypt_message(
  handshake: HandshakeState,
  frames: &[Vec<u8>],
) -> Result<Zeroizing<Vec<u8>>, FileError> {
  let mut transport = handshake.into_transport_mode()?;
  let mut reassembler = Reassembler::default();
  let mut plaintext = Zeroizing::new(vec![0u8; MAX_NOISE_MESSAGE]);
  let mut message = None;
  for frame in frames {
    if message.is_some() {
      return Err(FileError::Damaged);
    }
    let length = transport
      .read_message(frame, &mut plaintext)
      .map_err(|_| FileError::Damaged)?;
    message = reassembler
      .push(&plaintext[..length])
      .map_err(|_| FileError::Damaged)?;
  }
  message.ok_or(FileError::Damaged)
}

#[cfg(test)]
mod tests;
