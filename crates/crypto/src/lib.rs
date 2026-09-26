//! Master-key derivation and authenticated encryption for the vault.
//!
//! This crate owns exactly two responsibilities and nothing else:
//! 1. Deriving a 256-bit key from a user's master password (Argon2id).
//! 2. Encrypting/decrypting opaque byte payloads (XChaCha20-Poly1305).
//!
//! It has no knowledge of the vault file format, entries, or TOTP/HOTP —
//! that's `vault-core`, `storage`, and `otp`'s job. Keeping this crate
//! ignorant of everything above it is deliberate: the one thing in this
//! codebase that must be trivially reviewable in isolation is "how are
//! secrets encrypted," and that gets harder every time this crate learns
//! about something outside its actual job.

use std::{fmt, ops::RangeInclusive};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
  XChaCha20Poly1305,
  aead::{Aead, KeyInit, Payload},
};
use ctutils::{Choice, CtEq};
use zeroize::Zeroizing;

pub const SALT_LEN: usize = 16;
pub const KEY_LEN: usize = 32;
/// `XChaCha20-Poly1305`'s extended nonce length.
pub const NONCE_LEN: usize = 24;

/// Argon2id cost parameters, validated against explicit bounds.
///
/// The only way to obtain one is [`KdfParams::new`] (or one of the
/// named constants), so every value in circulation — including one
/// parsed from an attacker-influenced vault header — is guaranteed to
/// be inside the bounds below *before* anything tries to derive a key
/// with it. Without an upper bound, a corrupted or crafted header
/// (`t_cost = u32::MAX`) turns "unlock" into an unbounded hang.
///
/// **Lower bounds** are OWASP's Argon2id minimum (19 MiB, 2 passes,
/// 1 lane) — the weakest parameters this app has ever written, so every
/// existing vault still opens. **Upper bounds** leave headroom for
/// RFC 9106's first recommended option (2 GiB) and several times the
/// current default work, while still bounding the worst case to tens
/// of seconds instead of forever.
///
/// The defaults ([`KdfParams::RECOMMENDED`]) are deliberately heavier
/// than OWASP's *online authentication* baseline: that baseline exists
/// to protect a login server from `DoS` under concurrent load, which
/// doesn't apply here — this KDF runs once, locally, to unlock a single
/// vault.
///
/// **`p_cost` is 1.** The `argon2` crate (0.6) fills lanes one after
/// another unless its optional rayon-backed `parallel` feature is
/// enabled, which this workspace does not do (it would add a thread
/// pool to the dependency tree). Without that feature, raising `p_cost`
/// only splits the same memory into more sequentially-processed lanes:
/// the unlock takes as long and the attacker's per-guess cost does not
/// grow. Enabling `parallel` with `p_cost = 4` could buy several times
/// more memory × passes within the same unlock latency — worth
/// benchmarking on target hardware before changing the defaults.
///
/// Debug builds use exactly the same parameters. What used to make them
/// painfully slow under `pnpm tauri dev` (~15x) was unoptimized Argon2
/// code, so the root `Cargo.toml` compiles `argon2` and `blake2` with
/// `opt-level = 3` even in the dev profile instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
  m_cost_kib: u32,
  t_cost: u32,
  p_cost: u32,
}

/// Which Argon2 cost parameter a [`KdfParamsError`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfParameter {
  MemoryKib,
  Iterations,
  Parallelism,
}

impl fmt::Display for KdfParameter {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Self::MemoryKib => "Argon2 memory cost (KiB)",
      Self::Iterations => "Argon2 iteration count",
      Self::Parallelism => "Argon2 parallelism",
    })
  }
}

/// A KDF cost parameter outside [`KdfParams`]' supported bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unsupported {parameter}: {value} (supported range {min}..={max})")]
pub struct KdfParamsError {
  pub parameter: KdfParameter,
  pub value: u32,
  pub min: u32,
  pub max: u32,
}

impl KdfParams {
  /// Supported memory cost, in KiB: 19 MiB ..= 2 GiB.
  pub const M_COST_KIB: RangeInclusive<u32> = 19 * 1024..=2 * 1024 * 1024;
  /// Supported number of passes.
  pub const T_COST: RangeInclusive<u32> = 2..=16;
  /// Supported number of lanes.
  pub const P_COST: RangeInclusive<u32> = 1..=16;

  /// The weakest parameters accepted: OWASP's Argon2id minimum. Fast
  /// enough for tests; never what a new vault is created with.
  pub const MINIMUM: Self = Self {
    m_cost_kib: *Self::M_COST_KIB.start(),
    t_cost: *Self::T_COST.start(),
    p_cost: *Self::P_COST.start(),
  };

  /// What new vaults are created with, and what weaker vaults are
  /// upgraded to on unlock: 256 MiB, 3 passes, 1 lane (~0.5 s in a
  /// release build on a desktop CPU — re-measure on target hardware
  /// before changing).
  pub const RECOMMENDED: Self = Self {
    m_cost_kib: 256 * 1024,
    t_cost: 3,
    p_cost: 1,
  };

  /// Validate a set of cost parameters.
  ///
  /// # Errors
  ///
  /// Returns [`KdfParamsError`] naming the first parameter that lies
  /// outside [`Self::M_COST_KIB`], [`Self::T_COST`] or [`Self::P_COST`].
  pub fn new(m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<Self, KdfParamsError> {
    check(KdfParameter::MemoryKib, m_cost_kib, &Self::M_COST_KIB)?;
    check(KdfParameter::Iterations, t_cost, &Self::T_COST)?;
    check(KdfParameter::Parallelism, p_cost, &Self::P_COST)?;
    Ok(Self {
      m_cost_kib,
      t_cost,
      p_cost,
    })
  }

  #[must_use]
  pub const fn m_cost_kib(&self) -> u32 {
    self.m_cost_kib
  }

  #[must_use]
  pub const fn t_cost(&self) -> u32 {
    self.t_cost
  }

  #[must_use]
  pub const fn p_cost(&self) -> u32 {
    self.p_cost
  }

  /// Whether `self` is strictly weaker than `other`: no parameter is
  /// higher and at least one is lower. Parameters that are higher in
  /// one dimension and lower in another are *not* weaker — replacing
  /// them with `other` would downgrade that dimension.
  #[must_use]
  pub fn is_weaker_than(&self, other: &Self) -> bool {
    self != other
      && self.m_cost_kib <= other.m_cost_kib
      && self.t_cost <= other.t_cost
      && self.p_cost <= other.p_cost
  }

  fn to_argon2(self) -> Result<Params, CryptoError> {
    Params::new(self.m_cost_kib, self.t_cost, self.p_cost, Some(KEY_LEN)).map_err(CryptoError::Kdf)
  }
}

impl Default for KdfParams {
  fn default() -> Self {
    Self::RECOMMENDED
  }
}

fn check(
  parameter: KdfParameter,
  value: u32,
  bounds: &RangeInclusive<u32>,
) -> Result<(), KdfParamsError> {
  if bounds.contains(&value) {
    Ok(())
  } else {
    Err(KdfParamsError {
      parameter,
      value,
      min: *bounds.start(),
      max: *bounds.end(),
    })
  }
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
  #[error("key derivation failed")]
  Kdf(#[source] argon2::Error),
  #[error("encryption failed")]
  Encrypt,
  /// Deliberately one error for "wrong password", "tampered or
  /// corrupted ciphertext" and "tampered associated data" — the caller
  /// must not be able to distinguish them, since that distinction is
  /// itself an oracle an attacker could exploit.
  #[error("decryption failed — wrong password or corrupted vault")]
  Decrypt,
  #[error("failed to read system randomness")]
  Rng(#[source] getrandom::Error),
}

/// A 256-bit key derived from the master password.
///
/// No `Debug`, `Display`, `Clone`, or `PartialEq` impl — on purpose.
/// Printing it is a leak and copying it multiplies what has to be
/// scrubbed. Compare two keys with [`CtEq`], which runs in constant
/// time; the only other thing you can do with a `MasterKey` is pass it
/// to [`encrypt`] or [`decrypt`].
///
/// The key bytes live on the heap and are zeroed on drop, so moving a
/// `MasterKey` around (into a vault, a `Result`, an `Option`) copies a
/// pointer rather than leaving stale copies of the key on the stack.
pub struct MasterKey(Box<Zeroizing<[u8; KEY_LEN]>>);

impl MasterKey {
  fn bytes(&self) -> &[u8; KEY_LEN] {
    &self.0
  }

  fn cipher(&self) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(self.bytes().into())
  }
}

impl CtEq for MasterKey {
  fn ct_eq(&self, other: &Self) -> Choice {
    self.bytes().ct_eq(other.bytes())
  }
}

/// A random, non-secret salt. Safe to store alongside the ciphertext.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Salt(pub [u8; SALT_LEN]);

/// Generate a fresh random salt for a new vault.
///
/// # Errors
///
/// Returns [`CryptoError::Rng`] if the operating system's random number
/// generator cannot provide random bytes.
pub fn generate_salt() -> Result<Salt, CryptoError> {
  let mut bytes = [0u8; SALT_LEN];
  getrandom::fill(&mut bytes).map_err(CryptoError::Rng)?;
  Ok(Salt(bytes))
}

/// A fresh random nonce for exactly one [`encrypt`] call.
///
/// Deliberately neither `Clone` nor `Copy`, and only constructible
/// through [`Nonce::generate`]: [`encrypt`] takes it by value, so the
/// type system rules out ever encrypting twice under the same nonce
/// (which would be a full break of AEAD confidentiality). Callers read
/// its bytes with [`Nonce::as_bytes`] *before* encrypting, to put them
/// into whatever header the ciphertext is bound to.
pub struct Nonce([u8; NONCE_LEN]);

impl Nonce {
  /// # Errors
  ///
  /// Returns [`CryptoError::Rng`] if the operating system's random
  /// number generator cannot provide random bytes.
  pub fn generate() -> Result<Self, CryptoError> {
    let mut bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut bytes).map_err(CryptoError::Rng)?;
    Ok(Self(bytes))
  }

  #[must_use]
  pub const fn as_bytes(&self) -> &[u8; NONCE_LEN] {
    &self.0
  }

  /// Spends the nonce: only [`encrypt`] calls this.
  const fn into_bytes(self) -> [u8; NONCE_LEN] {
    self.0
  }
}

/// Derive the master key from a password and salt using Argon2id
/// (version 0x13).
///
/// `password` is taken as a byte slice rather than a `String` so the
/// caller controls its own zeroization of the raw password input; this
/// function does not take ownership of it and cannot zero it for you.
///
/// # Errors
///
/// Returns [`CryptoError::Kdf`] if Argon2 rejects the parameters or the
/// derivation fails.
pub fn derive_key(
  password: &[u8],
  salt: &Salt,
  params: &KdfParams,
) -> Result<MasterKey, CryptoError> {
  let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.to_argon2()?);

  let mut key = Box::new(Zeroizing::new([0u8; KEY_LEN]));
  argon2
    .hash_password_into(password, &salt.0, key.as_mut_slice())
    .map_err(CryptoError::Kdf)?;

  Ok(MasterKey(key))
}

/// Encrypt `plaintext` under `key` and `nonce`, authenticating
/// `associated_data` along with it.
///
/// `associated_data` is not encrypted and not part of the output, but
/// [`decrypt`] fails unless it is given byte-for-byte the same value —
/// this is how a caller binds a plaintext header (format version, KDF
/// parameters, …) to the ciphertext it describes.
///
/// A fresh random nonce per call is what makes it safe to call this on
/// every vault write without any counter/state to manage — `XChaCha`'s
/// 24-byte nonce space makes random-nonce collisions astronomically
/// unlikely. [`Nonce`] enforces the "fresh" part.
///
/// # Errors
///
/// Returns [`CryptoError::Encrypt`] if the plaintext cannot be
/// encrypted (it or the associated data exceeds the cipher's limits).
pub fn encrypt(
  key: &MasterKey,
  nonce: Nonce,
  plaintext: &[u8],
  associated_data: &[u8],
) -> Result<Vec<u8>, CryptoError> {
  key
    .cipher()
    .encrypt(
      &nonce.into_bytes().into(),
      Payload {
        msg: plaintext,
        aad: associated_data,
      },
    )
    .map_err(|_| CryptoError::Encrypt)
}

/// Decrypt `ciphertext` under `key`, verifying it together with the
/// `associated_data` it was encrypted with.
///
/// # Errors
///
/// Returns [`CryptoError::Decrypt`] if the ciphertext or the associated
/// data fails authentication.
pub fn decrypt(
  key: &MasterKey,
  nonce: &[u8; NONCE_LEN],
  ciphertext: &[u8],
  associated_data: &[u8],
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
  key
    .cipher()
    .decrypt(
      nonce.into(),
      Payload {
        msg: ciphertext,
        aad: associated_data,
      },
    )
    .map(Zeroizing::new)
    .map_err(|_| CryptoError::Decrypt)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
      write!(out, "{byte:02x}").unwrap();
      out
    })
  }

  fn test_key(password: &[u8], salt: &Salt) -> MasterKey {
    derive_key(password, salt, &KdfParams::MINIMUM).expect("derivation should succeed")
  }

  // ---- known-answer tests --------------------------------------------

  /// Expected value computed independently with the reference
  /// implementation (`argon2 somesaltsomesalt -id -t 2 -k 19456 -p 1
  /// -l 32 -r`, phc-winner-argon2) and cross-checked with argon2-cffi.
  /// Pins the algorithm (Argon2id), the version (0x13) and the output
  /// length, not just "some key came out".
  #[test]
  fn argon2id_known_answer() {
    let key = test_key(b"password", &Salt(*b"somesaltsomesalt"));
    assert_eq!(
      hex(key.bytes()),
      "2b5dc4054886ec957ef59c73b661c54dd6fb274590b278f657c6d96aac8fa6d1"
    );
  }

  /// draft-irtf-cfrg-xchacha-03, Appendix A.3.1 (cross-checked against
  /// libsodium's `crypto_aead_xchacha20poly1305_ietf_encrypt`).
  #[test]
  fn xchacha20poly1305_known_answer() {
    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip \
                      for the future, sunscreen would be it.";
    let aad = [
      0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    ];
    let key_bytes: [u8; KEY_LEN] = std::array::from_fn(|i| 0x80 + u8::try_from(i).unwrap());
    let nonce_bytes: [u8; NONCE_LEN] = std::array::from_fn(|i| 0x40 + u8::try_from(i).unwrap());
    let key = MasterKey(Box::new(Zeroizing::new(key_bytes)));

    let ciphertext = encrypt(&key, Nonce(nonce_bytes), plaintext, &aad).unwrap();

    assert_eq!(
      hex(&ciphertext),
      "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f\
       4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f\
       76b2383565d3fff921f9664c97637da9768812f615c68b13b52ec0875924c1c7987947deafd8780acf49"
    );
    assert_eq!(
      &*decrypt(&key, &nonce_bytes, &ciphertext, &aad).unwrap(),
      plaintext
    );
  }

  // ---- KDF parameters ------------------------------------------------

  #[test]
  fn recommended_params_are_the_default_and_within_bounds() {
    let recommended = KdfParams::RECOMMENDED;
    assert_eq!(KdfParams::default(), recommended);
    assert_eq!(
      KdfParams::new(
        recommended.m_cost_kib(),
        recommended.t_cost(),
        recommended.p_cost()
      ),
      Ok(recommended)
    );
    assert_eq!(
      (
        recommended.m_cost_kib(),
        recommended.t_cost(),
        recommended.p_cost()
      ),
      (256 * 1024, 3, 1),
      "changing the shipped KDF cost must be a deliberate, reviewed edit"
    );
    assert!(recommended.to_argon2().is_ok(), "argon2 must accept them");
  }

  #[test]
  fn minimum_params_are_within_bounds_and_accepted_by_argon2() {
    let min = KdfParams::MINIMUM;
    assert_eq!(
      KdfParams::new(min.m_cost_kib(), min.t_cost(), min.p_cost()),
      Ok(min)
    );
    assert!(min.to_argon2().is_ok());
    assert!(min.is_weaker_than(&KdfParams::RECOMMENDED));
  }

  #[test]
  fn out_of_bounds_params_are_rejected_before_any_derivation() {
    let min = KdfParams::MINIMUM;
    assert_eq!(
      KdfParams::new(min.m_cost_kib(), u32::MAX, min.p_cost()),
      Err(KdfParamsError {
        parameter: KdfParameter::Iterations,
        value: u32::MAX,
        min: 2,
        max: 16,
      })
    );
    assert!(matches!(
      KdfParams::new(u32::MAX, min.t_cost(), min.p_cost()),
      Err(KdfParamsError {
        parameter: KdfParameter::MemoryKib,
        ..
      })
    ));
    assert!(matches!(
      KdfParams::new(8 * 1024, min.t_cost(), min.p_cost()),
      Err(KdfParamsError {
        parameter: KdfParameter::MemoryKib,
        ..
      })
    ));
    assert!(matches!(
      KdfParams::new(min.m_cost_kib(), 1, min.p_cost()),
      Err(KdfParamsError {
        parameter: KdfParameter::Iterations,
        ..
      })
    ));
    assert!(matches!(
      KdfParams::new(min.m_cost_kib(), min.t_cost(), 0),
      Err(KdfParamsError {
        parameter: KdfParameter::Parallelism,
        ..
      })
    ));
    assert!(matches!(
      KdfParams::new(min.m_cost_kib(), min.t_cost(), 17),
      Err(KdfParamsError {
        parameter: KdfParameter::Parallelism,
        ..
      })
    ));
  }

  #[test]
  fn every_bound_combination_is_accepted_by_argon2() {
    for m in [*KdfParams::M_COST_KIB.start(), *KdfParams::M_COST_KIB.end()] {
      for t in [*KdfParams::T_COST.start(), *KdfParams::T_COST.end()] {
        for p in [*KdfParams::P_COST.start(), *KdfParams::P_COST.end()] {
          let params = KdfParams::new(m, t, p).unwrap();
          assert!(params.to_argon2().is_ok(), "m={m} t={t} p={p}");
        }
      }
    }
  }

  #[test]
  fn weaker_than_is_strict_dominance() {
    let base = KdfParams::new(64 * 1024, 3, 1).unwrap();
    let more_memory = KdfParams::new(128 * 1024, 3, 1).unwrap();
    let more_memory_fewer_passes = KdfParams::new(128 * 1024, 2, 1).unwrap();

    assert!(base.is_weaker_than(&more_memory));
    assert!(!more_memory.is_weaker_than(&base));
    assert!(!base.is_weaker_than(&base), "equal is not weaker");
    assert!(
      !base.is_weaker_than(&more_memory_fewer_passes),
      "incomparable params must not count as weaker (upgrading would downgrade t_cost)"
    );
  }

  // ---- AEAD ------------------------------------------------------------

  #[test]
  fn round_trip() {
    let salt = generate_salt().unwrap();
    let key = test_key(b"correct horse battery staple", &salt);
    let nonce = Nonce::generate().unwrap();
    let nonce_bytes = *nonce.as_bytes();

    let plaintext = b"a totp secret, allegedly";
    let ciphertext = encrypt(&key, nonce, plaintext, b"header").unwrap();
    let decrypted = decrypt(&key, &nonce_bytes, &ciphertext, b"header").unwrap();

    assert_eq!(&*decrypted, plaintext);
  }

  #[test]
  fn wrong_password_fails_to_decrypt() {
    let salt = generate_salt().unwrap();
    let key = test_key(b"correct password", &salt);
    let wrong_key = test_key(b"wrong password", &salt);
    let nonce = Nonce::generate().unwrap();
    let nonce_bytes = *nonce.as_bytes();

    let ciphertext = encrypt(&key, nonce, b"secret", b"").unwrap();

    assert!(matches!(
      decrypt(&wrong_key, &nonce_bytes, &ciphertext, b""),
      Err(CryptoError::Decrypt)
    ));
  }

  #[test]
  fn tampered_ciphertext_is_rejected() {
    let key = test_key(b"password", &generate_salt().unwrap());
    let nonce = Nonce::generate().unwrap();
    let nonce_bytes = *nonce.as_bytes();

    let mut ciphertext = encrypt(&key, nonce, b"secret payload", b"").unwrap();
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0x01; // flip one bit

    assert!(
      matches!(
        decrypt(&key, &nonce_bytes, &ciphertext, b""),
        Err(CryptoError::Decrypt)
      ),
      "AEAD must reject tampered ciphertext, not silently return garbage"
    );
  }

  #[test]
  fn tampered_nonce_is_rejected() {
    let key = test_key(b"password", &generate_salt().unwrap());
    let nonce = Nonce::generate().unwrap();
    let mut nonce_bytes = *nonce.as_bytes();

    let ciphertext = encrypt(&key, nonce, b"secret payload", b"").unwrap();
    nonce_bytes[0] ^= 0x01;

    assert!(decrypt(&key, &nonce_bytes, &ciphertext, b"").is_err());
  }

  #[test]
  fn tampered_associated_data_is_rejected() {
    let key = test_key(b"password", &generate_salt().unwrap());
    let nonce = Nonce::generate().unwrap();
    let nonce_bytes = *nonce.as_bytes();

    let ciphertext = encrypt(&key, nonce, b"secret payload", b"version 2").unwrap();

    assert!(matches!(
      decrypt(&key, &nonce_bytes, &ciphertext, b"version 1"),
      Err(CryptoError::Decrypt)
    ));
    assert!(decrypt(&key, &nonce_bytes, &ciphertext, b"").is_err());
  }

  #[test]
  fn fresh_nonces_differ_across_calls() {
    let a = Nonce::generate().unwrap();
    let b = Nonce::generate().unwrap();
    assert_ne!(
      a.as_bytes(),
      b.as_bytes(),
      "nonce reuse under the same key would break AEAD confidentiality"
    );
  }

  #[test]
  fn different_salts_yield_different_keys() {
    let key_a = test_key(b"same password", &generate_salt().unwrap());
    let key_b = test_key(b"same password", &generate_salt().unwrap());
    assert!(!key_a.ct_eq(&key_b).to_bool());
  }

  #[test]
  fn same_password_and_salt_yield_the_same_key() {
    let salt = generate_salt().unwrap();
    assert!(
      test_key(b"pw", &salt)
        .ct_eq(&test_key(b"pw", &salt))
        .to_bool()
    );
    assert!(
      !test_key(b"pw", &salt)
        .ct_eq(&test_key(b"pW", &salt))
        .to_bool()
    );
  }

  #[test]
  fn salts_are_not_all_zero_and_differ_across_calls() {
    let a = generate_salt().unwrap();
    let b = generate_salt().unwrap();
    assert_ne!(a.0, [0u8; SALT_LEN]);
    assert_ne!(a, b);
  }
}
