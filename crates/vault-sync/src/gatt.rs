//! The Bluetooth LE byte stream, without I/O.
//!
//! A sync session is a byte stream each way. Over GATT, the central
//! writes it to the [`RX_UUID`] characteristic and the peripheral
//! notifies it on [`TX_UUID`]; both cut it into chunks that fit one ATT
//! packet. Each chunk starts with a one-byte sequence number, which
//! wraps: a chunk that arrives out of order — possible when `BlueZ` spreads
//! GATT traffic over several enhanced ATT channels — ends the session
//! instead of corrupting it (the Noise layer would refuse it anyway, but
//! later and less clearly). The chunk just received, arriving again, is
//! no such chunk: a write the central's Bluetooth stack reported as
//! failed, and sent again, may have arrived the first time too. It is
//! dropped.

use std::{collections::HashMap, hash::BuildHasher};

use uuid::Uuid;
use vault_core::{DeviceIdentity, PublicKey};

use crate::{
  invitation::TOKEN_LEN,
  protocol::Snapshot,
  session::{Mode, Output, Session, SessionError},
};

/// Arsu's GATT service.
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x8a8f_11c2_b37f_496e_97a9_b1ba_6eb6_ee78);

/// The characteristic the central writes to (write with response).
pub const RX_UUID: Uuid = Uuid::from_u128(0xd38b_2aeb_d8f3_4c2d_bcda_8300_b7fa_5d42);

/// The characteristic the peripheral notifies on.
pub const TX_UUID: Uuid = Uuid::from_u128(0x4590_6ec1_7669_42a7_a423_266f_d299_5885);

/// The smallest ATT MTU (Bluetooth Core spec), which every link has.
pub const MIN_ATT_MTU: usize = 23;

/// Bytes of an ATT packet taken by its opcode and handle.
const ATT_HEADER_LEN: usize = 3;

/// Longest attribute value (Bluetooth Core spec), whatever the MTU.
const MAX_ATTRIBUTE_VALUE: usize = 512;

/// Bytes of a chunk taken by its sequence number.
const SEQUENCE_LEN: usize = 1;

/// What a peripheral advertises, a computer or a phone
/// alike. Each fits a legacy advertising packet (31 bytes), the only
/// kind phones scan for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advertising {
  /// Arsu's service, nothing else: no name, no device data.
  Sync,
  /// An invitation's token, as the service's data, so the joining device
  /// finds the device that shows *its* invitation. It replaces the
  /// service list: both would not fit.
  Pairing([u8; TOKEN_LEN]),
}

/// Whether a scanned device is an Arsu peripheral: it lists Arsu's
/// service (waiting to sync) or carries data for it (waiting to pair).
#[must_use]
pub fn advertises_arsu<'a, S: BuildHasher>(
  services: impl IntoIterator<Item = &'a Uuid>,
  service_data: &HashMap<Uuid, Vec<u8>, S>,
) -> bool {
  services.into_iter().any(|uuid| *uuid == SERVICE_UUID) || service_data.contains_key(&SERVICE_UUID)
}

/// The invitation token an Arsu peripheral in pairing mode advertises
///, read from a scanned device's service data.
#[must_use]
pub fn token_of<S: BuildHasher>(
  service_data: &HashMap<Uuid, Vec<u8>, S>,
) -> Option<[u8; TOKEN_LEN]> {
  service_data
    .get(&SERVICE_UUID)
    .and_then(|data| <[u8; TOKEN_LEN]>::try_from(data.as_slice()).ok())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GattError {
  #[error("a Bluetooth packet arrived empty")]
  EmptyChunk,
  #[error("Bluetooth packets arrived out of order: try again")]
  OutOfOrder,
}

/// Cuts the outgoing byte stream into chunks.
#[derive(Debug)]
pub struct Chunker {
  payload_len: usize,
  next: u8,
}

impl Chunker {
  /// A chunker for a link with this ATT MTU (below the minimum counts as
  /// the minimum).
  #[must_use]
  pub fn new(att_mtu: usize) -> Self {
    let value_len = (att_mtu.max(MIN_ATT_MTU) - ATT_HEADER_LEN).min(MAX_ATTRIBUTE_VALUE);
    Self {
      payload_len: value_len - SEQUENCE_LEN,
      next: 0,
    }
  }

  /// The bytes of a chunk after its sequence number.
  #[must_use]
  pub fn payload_len(&self) -> usize {
    self.payload_len
  }

  /// `bytes` as chunks, numbered on from the previous call.
  pub fn chunks(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes
      .chunks(self.payload_len)
      .map(|payload| {
        let mut chunk = Vec::with_capacity(SEQUENCE_LEN + payload.len());
        chunk.push(self.next);
        chunk.extend_from_slice(payload);
        self.next = self.next.wrapping_add(1);
        chunk
      })
      .collect()
  }
}

/// Checks and unwraps incoming chunks.
#[derive(Debug, Default)]
pub struct Dechunker {
  expected: u8,
  /// The last chunk taken, whole: a copy of it arriving next is a resent
  /// write, not a new chunk.
  last: Option<Vec<u8>>,
}

impl Dechunker {
  /// The bytes a chunk carries; `None` for the chunk just taken, arriving
  /// again (a write sent twice), which carries nothing new.
  ///
  /// # Errors
  ///
  /// [`GattError::EmptyChunk`] for a chunk without even a sequence
  /// number; [`GattError::OutOfOrder`] for one not next in sequence. The
  /// session cannot continue after either.
  pub fn push<'chunk>(&mut self, chunk: &'chunk [u8]) -> Result<Option<&'chunk [u8]>, GattError> {
    let (&sequence, payload) = chunk.split_first().ok_or(GattError::EmptyChunk)?;
    if sequence != self.expected {
      if self.last.as_deref() == Some(chunk) {
        return Ok(None);
      }
      return Err(GattError::OutOfOrder);
    }
    self.expected = self.expected.wrapping_add(1);
    self.last = Some(chunk.to_vec());
    Ok(Some(payload))
  }
}

/// What the Bluetooth code must do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkOutput {
  /// Send this chunk: the central writes it to [`RX_UUID`], the
  /// peripheral notifies it on [`TX_UUID`].
  Chunk(Vec<u8>),
  /// As [`Output::Received`]: merge, save, then
  /// [`GattLink::acknowledge`].
  Received {
    peer: PublicKey,
    snapshot: Box<Snapshot>,
  },
  /// As [`Output::Finished`]: disconnect.
  Finished,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
  #[error(transparent)]
  Gatt(#[from] GattError),
  #[error(transparent)]
  Session(#[from] SessionError),
}

/// A sync or pairing [`Session`] over GATT chunks: what arrives is one
/// chunk at a time, what leaves is one chunk per output. The Bluetooth
/// code only moves chunks.
pub struct GattLink {
  session: Session,
  chunker: Chunker,
  dechunker: Dechunker,
}

impl GattLink {
  /// Starts a session over a link with this ATT MTU.
  ///
  /// # Errors
  ///
  /// As [`Session::new`].
  pub fn new(
    mode: Mode,
    identity: &DeviceIdentity,
    snapshot: Snapshot,
    att_mtu: usize,
  ) -> Result<(Self, Vec<LinkOutput>), LinkError> {
    let (session, outputs) = Session::new(mode, identity, snapshot)?;
    let mut link = Self {
      session,
      chunker: Chunker::new(att_mtu),
      dechunker: Dechunker::default(),
    };
    let outputs = link.translate(outputs);
    Ok((link, outputs))
  }

  /// Takes one chunk from the peer.
  ///
  /// # Errors
  ///
  /// [`LinkError::Gatt`] for a chunk out of order, or any
  /// [`SessionError`]. The link cannot continue after an error.
  pub fn receive(&mut self, chunk: &[u8]) -> Result<Vec<LinkOutput>, LinkError> {
    let Some(bytes) = self.dechunker.push(chunk)? else {
      return Ok(Vec::new());
    };
    let outputs = self.session.receive(bytes)?;
    Ok(self.translate(outputs))
  }

  /// As [`Session::acknowledge`].
  ///
  /// # Errors
  ///
  /// As [`Session::acknowledge`].
  pub fn acknowledge(&mut self) -> Result<Vec<LinkOutput>, LinkError> {
    let outputs = self.session.acknowledge()?;
    Ok(self.translate(outputs))
  }

  /// The authenticated peer, once known.
  #[must_use]
  pub fn peer(&self) -> Option<PublicKey> {
    self.session.peer()
  }

  /// The session's outputs, with bytes to send cut into chunks.
  fn translate(&mut self, outputs: Vec<Output>) -> Vec<LinkOutput> {
    let mut translated = Vec::with_capacity(outputs.len());
    for output in outputs {
      match output {
        Output::Send(bytes) => translated.extend(
          self
            .chunker
            .chunks(&bytes)
            .into_iter()
            .map(LinkOutput::Chunk),
        ),
        Output::Received { peer, snapshot } => {
          translated.push(LinkOutput::Received { peer, snapshot });
        }
        Output::Finished => translated.push(LinkOutput::Finished),
      }
    }
    translated
  }
}

#[cfg(test)]
mod tests;
