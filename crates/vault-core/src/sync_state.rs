//! What the vault stores for sync: this device's identity and the set of
//! devices it trusts. The rules that act on them live
//! in the `vault-sync` crate; this is only their shape on disk.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SecretBytes;

/// Length of a Curve25519 public key, which is how devices are known.
pub const PUBLIC_KEY_LEN: usize = 32;

/// Sync's part of the vault. The default — no identity, no devices — is
/// a vault that has never synced, which is how payloads written before
/// sync existed read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
  /// This device's identity, once sync has been set up here.
  pub identity: Option<DeviceIdentity>,
  /// The devices allowed to sync this vault.
  pub devices: DeviceSet,
  /// Invitations this device has shown and that may still be used
  ///. Kept in the vault so that pairing by file survives
  /// the app being closed.
  #[serde(default)]
  pub invitations: Vec<PendingInvitation>,
}

/// An invitation shown by this device, not used yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingInvitation {
  /// The 256-bit pairing secret. Redacted from `Debug`, zeroized on
  /// drop.
  pub secret: SecretBytes,
  /// Unix seconds after which it can no longer be used.
  pub expires_at: u64,
}

/// This device's identity: the Noise static key pair it proves itself
/// with, and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
  /// The installation the identity was created on. A vault opened by
  /// another installation (a copied vault file) discards it and gets a
  /// new one, so two devices never share a key.
  pub installation_id: Uuid,
  /// Curve25519 private key. Redacted from `Debug`, zeroized on drop.
  pub private_key: SecretBytes,
  pub public_key: PublicKey,
  /// What other devices call this one ("Computer", "Pixel 8"…).
  pub name: String,
}

/// A device's Curve25519 public key: its identity for sync.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PublicKey(#[serde(with = "serde_bytes")] pub [u8; PUBLIC_KEY_LEN]);

impl PublicKey {
  /// The key's first eight bytes, as four groups of four hex digits
  /// (`ABCD 0123 4567 89EF`): for people to compare a device between
  /// screens and messages.
  #[must_use]
  pub fn fingerprint(&self) -> String {
    self.0[..8]
      .chunks(2)
      .map(|pair| format!("{:02X}{:02X}", pair[0], pair[1]))
      .collect::<Vec<_>>()
      .join(" ")
  }
}

impl fmt::Debug for PublicKey {
  /// The first eight bytes in hex: enough to tell keys apart in a log.
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("PublicKey(")?;
    for byte in &self.0[..8] {
      write!(f, "{byte:02x}")?;
    }
    f.write_str("…)")
  }
}

/// The devices of a vault: every device ever added, and the
/// ones removed since. Both lists only grow; a device is trusted when it
/// is a member and has not been removed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSet {
  pub members: Vec<Member>,
  pub removed: Vec<Removal>,
}

/// A device added to the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
  pub public_key: PublicKey,
  pub name: String,
  /// Unix seconds.
  pub added_at: u64,
}

/// A device removed from the vault: permanently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
  pub public_key: PublicKey,
  /// Unix seconds.
  pub removed_at: u64,
}

#[cfg(test)]
mod tests {
  use super::*;

  fn identity() -> DeviceIdentity {
    DeviceIdentity {
      installation_id: Uuid::from_u128(7),
      private_key: b"0123456789abcdef0123456789abcdef".to_vec().into(),
      public_key: PublicKey([0xab; PUBLIC_KEY_LEN]),
      name: "Computer".to_string(),
    }
  }

  #[test]
  fn a_fingerprint_is_the_keys_first_eight_bytes_in_four_groups() {
    let mut key = [0u8; PUBLIC_KEY_LEN];
    key[..8].copy_from_slice(&[0xab, 0xcd, 0x01, 0x23, 0x45, 0x67, 0x89, 0xef]);

    assert_eq!(PublicKey(key).fingerprint(), "ABCD 0123 4567 89EF");
  }

  fn round_trip<T: Serialize + for<'de> Deserialize<'de>>(value: &T) -> T {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).unwrap();
    ciborium::from_reader(bytes.as_slice()).unwrap()
  }

  #[test]
  fn sync_state_round_trips_through_cbor() {
    let state = SyncState {
      identity: Some(identity()),
      devices: DeviceSet {
        members: vec![Member {
          public_key: PublicKey([1; PUBLIC_KEY_LEN]),
          name: "Phone".to_string(),
          added_at: 10,
        }],
        removed: vec![Removal {
          public_key: PublicKey([2; PUBLIC_KEY_LEN]),
          removed_at: 20,
        }],
      },
      invitations: vec![PendingInvitation {
        secret: vec![5; 32].into(),
        expires_at: 30,
      }],
    };

    assert_eq!(round_trip(&state), state);
  }

  #[test]
  fn a_sync_state_without_invitations_reads_as_having_none() {
    #[derive(Serialize)]
    struct Earlier {
      identity: Option<DeviceIdentity>,
      devices: DeviceSet,
    }
    let mut bytes = Vec::new();
    ciborium::into_writer(
      &Earlier {
        identity: None,
        devices: DeviceSet::default(),
      },
      &mut bytes,
    )
    .unwrap();

    let state: SyncState = ciborium::from_reader(bytes.as_slice()).unwrap();

    assert!(state.invitations.is_empty());
  }

  #[test]
  fn debug_output_never_contains_an_invitation_secret() {
    let invitation = PendingInvitation {
      secret: b"pairing-secret-pairing-secret-32".to_vec().into(),
      expires_at: 1,
    };

    assert!(!format!("{invitation:?}").contains("pairing-secret"));
  }

  #[test]
  fn a_public_key_is_stored_as_a_byte_string() {
    let mut bytes = Vec::new();
    ciborium::into_writer(&PublicKey([0; PUBLIC_KEY_LEN]), &mut bytes).unwrap();

    // CBOR major type 2 (byte string), length 32: 0x58 0x20, then the key.
    assert_eq!(&bytes[..2], &[0x58, 0x20]);
    assert_eq!(bytes.len(), 2 + PUBLIC_KEY_LEN);
  }

  #[test]
  fn debug_output_never_contains_the_private_key() {
    let printed = format!("{:?}", identity());

    assert!(printed.contains("SecretBytes([REDACTED])"));
    assert!(!printed.contains("0123456789abcdef"));
    assert!(printed.contains("PublicKey(abababababababab…)"));
  }
}
