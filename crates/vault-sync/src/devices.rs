//! Which devices may sync a vault.
//!
//! A [`DeviceSet`] holds every device ever added and every device
//! removed. Both only grow, and merging two sets is their union, so
//! devices agree on the set whatever order they sync in. Removal is
//! permanent: a removed key is never trusted again, whichever device
//! says otherwise later.
//!
//! The set is only ever accepted from a peer that has just proved it
//! holds a trusted key; it is not signed.

use std::collections::BTreeMap;

use vault_core::{DeviceSet, Member, PublicKey, Removal};

/// Most devices a vault can trust at once.
pub const MAX_TRUSTED_DEVICES: usize = 64;

/// Most devices a vault can ever have known (trusted or removed), which
/// bounds what a peer can make this device store.
pub const MAX_KNOWN_DEVICES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeviceSetError {
  #[error("a vault can sync with at most {MAX_TRUSTED_DEVICES} devices")]
  TooManyDevices,
  #[error("this device was removed from the vault and must be paired again")]
  Removed,
}

/// Whether `key` may sync this vault.
#[must_use]
pub fn is_trusted(set: &DeviceSet, key: &PublicKey) -> bool {
  is_member(set, key) && !is_removed(set, key)
}

/// The trusted devices, by public key.
pub fn trusted(set: &DeviceSet) -> impl Iterator<Item = &Member> {
  set
    .members
    .iter()
    .filter(|member| !is_removed(set, &member.public_key))
}

/// Adds a device. Adding a device that is already a member keeps the
/// earlier of the two records, as a merge would.
///
/// # Errors
///
/// [`DeviceSetError::Removed`] if the key was removed before;
/// [`DeviceSetError::TooManyDevices`] past the limits.
pub fn add(set: &mut DeviceSet, member: Member) -> Result<(), DeviceSetError> {
  if is_removed(set, &member.public_key) {
    return Err(DeviceSetError::Removed);
  }
  let addition = DeviceSet {
    members: vec![member],
    removed: Vec::new(),
  };
  *set = merge(set, &addition)?;
  Ok(())
}

/// Removes a device, for good. Removing it again keeps the first time.
pub fn remove(set: &mut DeviceSet, key: PublicKey, now: u64) {
  if let Some(known) = set
    .removed
    .iter_mut()
    .find(|removal| removal.public_key == key)
  {
    known.removed_at = known.removed_at.min(now);
  } else {
    set.removed.push(Removal {
      public_key: key,
      removed_at: now,
    });
    set.removed.sort_by_key(|removal| removal.public_key);
  }
}

/// Two device sets as one: every member and every removal of either,
/// sorted by key. For a device both list, the record with the earliest
/// time wins (then the smaller name, so every device picks the same).
///
/// # Errors
///
/// [`DeviceSetError::TooManyDevices`] if the union is past the limits;
/// nothing is merged then.
pub fn merge(a: &DeviceSet, b: &DeviceSet) -> Result<DeviceSet, DeviceSetError> {
  let mut members: BTreeMap<PublicKey, &Member> = BTreeMap::new();
  for member in a.members.iter().chain(&b.members) {
    members
      .entry(member.public_key)
      .and_modify(|known| {
        if (member.added_at, &member.name) < (known.added_at, &known.name) {
          *known = member;
        }
      })
      .or_insert(member);
  }

  let mut removed: BTreeMap<PublicKey, u64> = BTreeMap::new();
  for removal in a.removed.iter().chain(&b.removed) {
    removed
      .entry(removal.public_key)
      .and_modify(|removed_at| *removed_at = (*removed_at).min(removal.removed_at))
      .or_insert(removal.removed_at);
  }

  let trusted = members
    .keys()
    .filter(|key| !removed.contains_key(key))
    .count();
  let known = members.len()
    + removed
      .keys()
      .filter(|key| !members.contains_key(key))
      .count();
  if trusted > MAX_TRUSTED_DEVICES || known > MAX_KNOWN_DEVICES {
    return Err(DeviceSetError::TooManyDevices);
  }

  Ok(DeviceSet {
    members: members.into_values().cloned().collect(),
    removed: removed
      .into_iter()
      .map(|(public_key, removed_at)| Removal {
        public_key,
        removed_at,
      })
      .collect(),
  })
}

fn is_member(set: &DeviceSet, key: &PublicKey) -> bool {
  set.members.iter().any(|member| member.public_key == *key)
}

/// Whether `key` was removed from the vault (it may never have been a
/// member here).
#[must_use]
pub fn is_removed(set: &DeviceSet, key: &PublicKey) -> bool {
  set.removed.iter().any(|removal| removal.public_key == *key)
}

#[cfg(test)]
mod tests;
