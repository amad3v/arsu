//! This device's sync identity over its lifetime.
//!
//! * [`reconcile`] runs before every sync operation. It discards an
//!   identity created by another installation (a copied vault file), so
//!   two devices never share a key, and creates one when there is none.
//!   It never touches the device set, so it cannot fail because the set
//!   is full — which would lock the user out of removing a device.
//! * [`list_self`] adds this device's own record to the device set, so
//!   that peers learn its name: before inviting or joining.
//! * [`forget_if_removed`] runs after every merge. A device that learns
//!   it was removed discards its identity; using sync again means
//!   pairing anew, with a new key.
//!
//! Discarding keeps the device set and the accounts, and drops the open
//! invitations with the identity they belonged to.

use uuid::Uuid;
use vault_core::{DeviceIdentity, Member, SyncState};

use crate::{
  devices::{self, DeviceSetError},
  noise,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
  #[error("set up sync on this device first")]
  NoIdentity,
  #[error(transparent)]
  Devices(#[from] DeviceSetError),
}

/// Makes sure this installation has its own identity in `state`, and
/// returns it. `name` is used only when a new identity is created.
/// Nothing in `state` changes if this fails.
///
/// # Errors
///
/// [`snow::Error`] if no key can be generated.
pub fn reconcile<'state>(
  state: &'state mut SyncState,
  installation_id: Uuid,
  name: &str,
) -> Result<&'state DeviceIdentity, snow::Error> {
  let current = state
    .identity
    .take_if(|identity| identity.installation_id == installation_id);
  let identity = if let Some(identity) = current {
    identity
  } else {
    // Generated before anything changes, so a failure changes nothing.
    let identity = noise::new_identity(installation_id, name.to_owned())?;
    if state.identity.take().is_some() {
      state.invitations.clear();
    }
    identity
  };
  Ok(state.identity.insert(identity))
}

/// Adds this device's own record (its key and name, added `now`) to the
/// device set, if it is not there yet.
///
/// # Errors
///
/// [`IdentityError::NoIdentity`] before [`reconcile`];
/// [`IdentityError::Devices`] if the set is full or this device was
/// removed.
pub fn list_self(state: &mut SyncState, now: u64) -> Result<(), IdentityError> {
  let identity = state.identity.as_ref().ok_or(IdentityError::NoIdentity)?;
  if devices::is_trusted(&state.devices, &identity.public_key) {
    return Ok(());
  }
  let member = Member {
    public_key: identity.public_key,
    name: identity.name.clone(),
    added_at: now,
  };
  Ok(devices::add(&mut state.devices, member)?)
}

/// Discards this device's identity if the device set says it was
/// removed. Returns whether it did.
pub fn forget_if_removed(state: &mut SyncState) -> bool {
  let removed = state
    .identity
    .as_ref()
    .is_some_and(|identity| devices::is_removed(&state.devices, &identity.public_key));
  if removed {
    state.identity = None;
    state.invitations.clear();
  }
  removed
}

#[cfg(test)]
mod tests;
