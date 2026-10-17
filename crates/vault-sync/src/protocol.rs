//! The application messages of a sync session.
//!
//! A session is two messages each way: each side sends a [`Snapshot`]
//! of its whole sync state, merges the one it receives, and answers
//! [`Message::Done`]. Messages are CBOR maps; an enum is encoded
//! externally tagged (`{"Snapshot": {...}}`), which serde decodes
//! without staging the content — seeds included — in an intermediate
//! buffer that would not be zeroized.
//!
//! Compatibility: the Noise prologue carries the protocol's major
//! version, so incompatible peers fail during the handshake. Within a
//! major version, fields may only be added, and unknown fields are
//! ignored.

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vault_core::{DeviceSet, Entry, Tombstone, VaultCoreError, cbor};
use zeroize::Zeroizing;

use crate::Replica;

/// The protocol version this build speaks.
pub const PROTOCOL_VERSION: u16 = 1;

/// Most tombstones a snapshot may carry.
pub const MAX_TOMBSTONES: usize = 100_000;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
  #[error("could not encode or decode a sync message")]
  Cbor(#[from] VaultCoreError),
  #[error("the other device speaks sync protocol {0}, this one {PROTOCOL_VERSION}: update Arsu")]
  UnsupportedProtocol(u16),
  #[error("the other device sent more than {MAX_TOMBSTONES} deleted entries")]
  TooManyTombstones,
}

/// One device's whole sync state, as sent to a peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
  pub protocol: u16,
  /// The sender's name for itself, shown in the summary and, when
  /// pairing, recorded in the device set.
  pub sender_name: String,
  pub vault_id: Uuid,
  pub entries: Vec<Entry>,
  pub tombstones: Vec<Tombstone>,
  pub devices: DeviceSet,
}

/// What travels in a session, after the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
  Snapshot(Box<Snapshot>),
  /// The sender has received and merged the peer's snapshot.
  Done,
}

impl Snapshot {
  /// A snapshot of this protocol version.
  #[must_use]
  pub fn new(sender_name: String, replica: Replica, devices: DeviceSet) -> Self {
    Self {
      protocol: PROTOCOL_VERSION,
      sender_name,
      vault_id: replica.vault_id,
      entries: replica.entries,
      tombstones: replica.tombstones,
      devices,
    }
  }

  /// The sender's name, the merge-relevant part and the device set,
  /// moved out rather than copied: the entries hold seeds.
  #[must_use]
  pub fn into_parts(self) -> (String, Replica, DeviceSet) {
    let replica = Replica {
      vault_id: self.vault_id,
      entries: self.entries,
      tombstones: self.tombstones,
    };
    (self.sender_name, replica, self.devices)
  }

  /// The merge-relevant part.
  #[must_use]
  pub fn replica(&self) -> Replica {
    Replica {
      vault_id: self.vault_id,
      entries: self.entries.clone(),
      tombstones: self.tombstones.clone(),
    }
  }
}

impl Message {
  /// The message as CBOR, in a zeroizing buffer.
  ///
  /// # Errors
  ///
  /// [`ProtocolError::Cbor`] if it cannot be encoded.
  pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>, ProtocolError> {
    Ok(cbor::to_zeroizing_vec(self)?)
  }

  /// Decodes and checks a message from a peer.
  ///
  /// # Errors
  ///
  /// [`ProtocolError::Cbor`] for anything that is not a message,
  /// [`ProtocolError::UnsupportedProtocol`] for a snapshot of another
  /// version, [`ProtocolError::TooManyTombstones`] past the limit.
  pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
    let message: Self = cbor::from_slice(bytes)?;
    if let Self::Snapshot(snapshot) = &message {
      if snapshot.protocol != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedProtocol(snapshot.protocol));
      }
      if snapshot.tombstones.len() > MAX_TOMBSTONES {
        return Err(ProtocolError::TooManyTombstones);
      }
    }
    Ok(message)
  }
}

#[cfg(test)]
mod tests;
