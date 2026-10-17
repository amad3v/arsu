//! The Noise suite sync uses, and device identities.

use snow::{Builder, HandshakeState, params::NoiseParams};
use uuid::Uuid;
use vault_core::{DeviceIdentity, PUBLIC_KEY_LEN, PublicKey};
use zeroize::Zeroizing;

/// Length of a pairing secret, used as a Noise PSK: 256 bits, as the
/// Noise spec requires of PSKs (§14).
pub const SECRET_LEN: usize = 32;

/// A pairing secret: random, single-use, zeroized on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingSecret(Zeroizing<[u8; SECRET_LEN]>);

impl PairingSecret {
  /// A fresh random secret.
  ///
  /// # Errors
  ///
  /// [`getrandom::Error`] if the system's random generator fails.
  pub fn generate() -> Result<Self, getrandom::Error> {
    let mut secret = Zeroizing::new([0u8; SECRET_LEN]);
    getrandom::fill(secret.as_mut_slice())?;
    Ok(Self(secret))
  }

  #[must_use]
  pub fn from_bytes(bytes: [u8; SECRET_LEN]) -> Self {
    Self(Zeroizing::new(bytes))
  }

  #[must_use]
  pub fn expose(&self) -> &[u8; SECRET_LEN] {
    &self.0
  }
}

impl std::fmt::Debug for PairingSecret {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("PairingSecret([REDACTED])")
  }
}

/// A new identity for this device: a fresh Curve25519 key pair.
///
/// # Errors
///
/// [`snow::Error`] if the system's random generator fails.
pub fn new_identity(installation_id: Uuid, name: String) -> Result<DeviceIdentity, snow::Error> {
  let keypair = Builder::new(params(Purpose::Session)).generate_keypair()?;
  let public_key = public_key(&keypair.public).ok_or(snow::Error::Dh)?;
  Ok(DeviceIdentity {
    installation_id,
    // Moved, not copied: the key's only buffer is now zeroized on drop.
    private_key: keypair.private.into(),
    public_key,
    name,
  })
}

/// What a handshake is for. Each purpose has its own pattern and
/// prologue, so a message meant for one is never accepted as another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
  /// A session between paired devices: `Noise_XX`.
  Session,
  /// A pairing session: `Noise_XXpsk3`.
  Pairing,
  /// A sync file: one-way `Noise_X`.
  File,
  /// A join file: one-way `Noise_Xpsk1`.
  Join,
}

impl Purpose {
  fn params(self) -> &'static str {
    match self {
      Self::Session => "Noise_XX_25519_ChaChaPoly_BLAKE2s",
      Self::Pairing => "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s",
      Self::File => "Noise_X_25519_ChaChaPoly_BLAKE2s",
      Self::Join => "Noise_Xpsk1_25519_ChaChaPoly_BLAKE2s",
    }
  }

  /// Which handshake message the pairing secret enters with, if any.
  fn psk_position(self) -> Option<u8> {
    match self {
      Self::Session | Self::File => None,
      Self::Pairing => Some(3),
      Self::Join => Some(1),
    }
  }

  fn prologue(self) -> &'static [u8] {
    match self {
      Self::Session => b"arsu-sync/1 session",
      Self::Pairing => b"arsu-sync/1 pairing",
      Self::File => b"arsu-sync/1 file",
      Self::Join => b"arsu-sync/1 join",
    }
  }
}

/// A handshake state for `purpose`, with this device's static key; the
/// pairing secret for the purposes that use one; and, for the sender of
/// a one-way message, the recipient's key.
pub(crate) fn handshake(
  purpose: Purpose,
  initiator: bool,
  identity: &DeviceIdentity,
  secret: Option<&PairingSecret>,
  recipient: Option<&PublicKey>,
) -> Result<HandshakeState, snow::Error> {
  let mut builder = Builder::new(params(purpose))
    .local_private_key(identity.private_key.expose_secret())?
    .prologue(purpose.prologue())?;
  if let (Some(position), Some(secret)) = (purpose.psk_position(), secret) {
    builder = builder.psk(position, secret.expose())?;
  }
  if let Some(recipient) = recipient {
    builder = builder.remote_public_key(&recipient.0)?;
  }
  if initiator {
    builder.build_initiator()
  } else {
    builder.build_responder()
  }
}

/// A peer's static key as sent in a handshake.
pub(crate) fn public_key(bytes: &[u8]) -> Option<PublicKey> {
  <[u8; PUBLIC_KEY_LEN]>::try_from(bytes).ok().map(PublicKey)
}

fn params(purpose: Purpose) -> NoiseParams {
  purpose
    .params()
    .parse()
    .expect("the pattern names above are valid Noise protocol names")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn identities_are_distinct_key_pairs() {
    let a = new_identity(Uuid::from_u128(1), "Computer".to_string()).unwrap();
    let b = new_identity(Uuid::from_u128(1), "Computer".to_string()).unwrap();

    assert_eq!(a.private_key.len(), 32);
    assert_ne!(a.public_key, b.public_key);
    assert_eq!(a.installation_id, Uuid::from_u128(1));
    assert_eq!(a.name, "Computer");
  }

  #[test]
  fn pairing_secrets_are_random_and_redacted() {
    let a = PairingSecret::generate().unwrap();
    let b = PairingSecret::generate().unwrap();

    assert_ne!(a, b);
    assert_eq!(format!("{a:?}"), "PairingSecret([REDACTED])");
  }

  #[test]
  fn every_purpose_has_a_valid_protocol_name() {
    for purpose in [
      Purpose::Session,
      Purpose::Pairing,
      Purpose::File,
      Purpose::Join,
    ] {
      let _ = params(purpose);
    }
  }
}
