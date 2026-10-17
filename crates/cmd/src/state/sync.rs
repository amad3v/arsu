//! Sync: this device's identity, its devices, invitations and sync
//! files.
//!
//! Every sync operation starts from [`AppState::sync_identity`], which
//! creates the identity on first use and renews it for a vault copied
//! from another installation, so that two devices never share a key.
//! Whatever brings a peer's snapshot — a file now, a Bluetooth session
//! later — ends in [`apply_snapshot`], the one place a merge is applied.

use storage::VaultStorage;
use vault_core::{DeviceIdentity, Member, PUBLIC_KEY_LEN, PublicKey, VaultPayload};
use vault_sync::{
  Replica, devices, file, identity,
  invitation::{self, Invitation},
  merge,
  protocol::Snapshot,
};

use super::AppState;
use crate::{
  clock::unix_now,
  dto::{SecretString, SyncDevice, SyncOverview, SyncSummary},
  error::AppError,
  files::UserFile,
};

/// What a device trusted through an invitation's QR code is called until
/// its own record arrives.
pub const INVITING_DEVICE_NAME: &str = "Inviting device";

/// The `added_at` of that placeholder record: later than any real one,
/// so that the inviting device's own record — the earliest wins when
/// device sets merge — always replaces it, whatever the two clocks say.
/// The interface shows such a device as waiting for its reply.
pub const PENDING_ADDED_AT: u64 = u64::MAX;

impl AppState {
  /// This device's sync identity, created or renewed as needed. The vault is saved when the identity changes.
  ///
  /// # Errors
  ///
  /// [`AppError::Locked`]; [`AppError::Storage`] if the installation id
  /// or the vault cannot be read or written; [`AppError::KeyGeneration`]
  /// if no key can be made.
  pub(crate) fn sync_identity(&self) -> Result<DeviceIdentity, AppError> {
    let installation_id = storage::installation_id(self.paths.installation_id())?;
    let storage = self.storage()?;
    let mut session = self.session();
    let current = &session.unlocked()?.payload.sync.identity;
    if let Some(identity) = current
      && identity.installation_id == installation_id
    {
      return Ok(identity.clone());
    }
    session.commit(&storage, VaultStorage::save, |payload| {
      let identity =
        identity::reconcile(&mut payload.sync, installation_id, &default_device_name())?;
      Ok(identity.clone())
    })
  }

  /// Whether this device is paired with at least one other device: the
  /// only kind it can sync with.
  ///
  /// # Errors
  ///
  /// As [`Self::sync_identity`].
  pub fn has_paired_devices(&self) -> Result<bool, AppError> {
    let identity = self.sync_identity()?;
    let session = self.session();
    let devices_here = &session.unlocked()?.payload.sync.devices;
    Ok(devices::trusted(devices_here).any(|member| member.public_key != identity.public_key))
  }

  /// This device and the other devices of the vault.
  ///
  /// # Errors
  ///
  /// As [`Self::sync_identity`].
  pub fn sync_overview(&self) -> Result<SyncOverview, AppError> {
    let identity = self.sync_identity()?;
    let mut session = self.session();
    session.record_activity();
    let devices_here = &session.unlocked()?.payload.sync.devices;
    let this_added_at = devices_here
      .members
      .iter()
      .find(|member| member.public_key == identity.public_key)
      .map_or(0, |member| member.added_at);
    let mut others: Vec<SyncDevice> = devices::trusted(devices_here)
      .filter(|member| member.public_key != identity.public_key)
      .map(|member| SyncDevice::new(&member.public_key, &member.name, member.added_at))
      .collect();
    others.sort_by(|a, b| (a.added_at, &a.id).cmp(&(b.added_at, &b.id)));
    Ok(SyncOverview {
      this_device: SyncDevice::new(&identity.public_key, &identity.name, this_added_at),
      devices: others,
    })
  }

  /// Opens an invitation from this device, listing this
  /// device in the device set so that the joining one learns its name.
  /// Returns it with the time it expires, in Unix seconds.
  ///
  /// # Errors
  ///
  /// As [`Self::sync_identity`]; [`AppError::Identity`] if the device
  /// set is full; `Randomness`; or an error if the vault cannot be
  /// saved.
  pub fn create_invitation(&self) -> Result<(Invitation, u64), AppError> {
    self.sync_identity()?;
    let now = unix_now()?;
    let storage = self.storage()?;
    let mut session = self.session();
    session.record_activity();
    // Bookkeeping, not an edit worth a backup.
    session.commit(&storage, VaultStorage::save_without_rotation, |payload| {
      identity::list_self(&mut payload.sync, now)?;
      let invitation = invitation::open(&mut payload.sync, now)?;
      Ok((invitation, now.saturating_add(invitation::LIFETIME)))
    })
  }

  /// Removes a trusted device from the vault, for good.
  ///
  /// # Errors
  ///
  /// [`AppError::InvalidDeviceId`], [`AppError::UnknownDevice`] for a key
  /// the vault does not trust, [`AppError::CannotRemoveThisDevice`], as
  /// [`Self::sync_identity`], or an error if the vault cannot be saved.
  pub fn remove_sync_device(&self, id: &str) -> Result<(), AppError> {
    let key = parse_device_id(id)?;
    let identity = self.sync_identity()?;
    if key == identity.public_key {
      return Err(AppError::CannotRemoveThisDevice);
    }
    let now = unix_now()?;
    let storage = self.storage()?;
    let mut session = self.session();
    session.record_activity();
    session.commit(&storage, VaultStorage::save, |payload| {
      if !devices::is_trusted(&payload.sync.devices, &key) {
        return Err(AppError::UnknownDevice);
      }
      devices::remove(&mut payload.sync.devices, key, now);
      Ok(())
    })
  }

  /// Checks that a sync file can be written for `recipient`: the vault is
  /// unlocked and trusts it. Run before asking where to save.
  ///
  /// # Errors
  ///
  /// [`AppError::InvalidDeviceId`], [`AppError::Locked`] or
  /// [`AppError::UntrustedDevice`].
  pub fn check_sync_recipient(&self, recipient: &str) -> Result<PublicKey, AppError> {
    let recipient = parse_device_id(recipient)?;
    let session = self.session();
    if devices::is_trusted(&session.unlocked()?.payload.sync.devices, &recipient) {
      Ok(recipient)
    } else {
      Err(AppError::UntrustedDevice)
    }
  }

  /// Writes a sync file for a trusted device.
  ///
  /// # Errors
  ///
  /// As [`Self::check_sync_recipient`] and [`Self::sync_identity`];
  /// [`AppError::SyncFile`]; or `FileWrite`.
  pub fn export_sync_file(&self, recipient: &str, file: &impl UserFile) -> Result<(), AppError> {
    let identity = self.sync_identity()?;
    let recipient = self.check_sync_recipient(recipient)?;
    let snapshot = {
      let mut session = self.session();
      session.record_activity();
      local_snapshot(&session.unlocked()?.payload, &identity)
    };
    file.write(&file::write_sync_file(&identity, recipient, snapshot)?)
  }

  /// Checks that a join file can be written for an invitation's QR code
  /// text: it is an invitation, the vault is unlocked, and the inviting
  /// device was not removed. Run before asking where to save.
  ///
  /// # Errors
  ///
  /// [`AppError::Invitation`], [`AppError::Locked`], or
  /// [`AppError::Devices`] (`DeviceRemoved`).
  pub fn check_join_invitation(&self, invitation: &SecretString) -> Result<Invitation, AppError> {
    let invitation = Invitation::from_qr_text(invitation.expose().trim())?;
    let session = self.session();
    if devices::is_removed(
      &session.unlocked()?.payload.sync.devices,
      &invitation.inviter,
    ) {
      return Err(AppError::Devices(devices::DeviceSetError::Removed));
    }
    Ok(invitation)
  }

  /// Writes a join file for the device that showed an invitation, read
  /// from its QR code. Once the file is written, this
  /// device trusts the inviting one, so that its reply can be read; until
  /// that reply, it is listed under a placeholder ([`PENDING_ADDED_AT`]).
  ///
  /// # Errors
  ///
  /// As [`Self::check_join_invitation`] and [`Self::sync_identity`];
  /// [`AppError::Identity`] if the device set is full;
  /// [`AppError::SyncFile`]; or `FileWrite` (nothing is trusted then).
  pub fn export_join_file(
    &self,
    invitation: &SecretString,
    file: &impl UserFile,
  ) -> Result<(), AppError> {
    let identity = self.sync_identity()?;
    let invitation = self.check_join_invitation(invitation)?;
    let now = unix_now()?;
    let storage = self.storage()?;
    let snapshot = {
      let mut session = self.session();
      session.record_activity();
      session.commit(&storage, VaultStorage::save_without_rotation, |payload| {
        identity::list_self(&mut payload.sync, now)?;
        Ok(local_snapshot(payload, &identity))
      })?
    };
    file.write(&file::write_join_file(&identity, &invitation, snapshot)?)?;
    let mut session = self.session();
    session.commit(&storage, VaultStorage::save, |payload| {
      devices::add(
        &mut payload.sync.devices,
        Member {
          public_key: invitation.inviter,
          name: INVITING_DEVICE_NAME.to_owned(),
          added_at: PENDING_ADDED_AT,
        },
      )?;
      Ok(())
    })
  }

  /// Reads a sync or join file, and merges it into the vault. The file is
  /// decrypted without holding the vault, which is checked again before
  /// the merge: the sender must still be trusted, or the invitation still
  /// open.
  ///
  /// # Errors
  ///
  /// `FileRead`, `NotAFile`, `FileTooLarge`, as [`Self::sync_identity`],
  /// [`AppError::SyncFile`], [`AppError::UntrustedDevice`],
  /// [`AppError::Devices`], or an error if the vault cannot be saved
  /// (nothing is merged then).
  pub fn import_sync_file(&self, file: &impl UserFile) -> Result<SyncSummary, AppError> {
    let bytes = file.read(file::MAX_FILE as u64)?;
    let identity = self.sync_identity()?;
    let now = unix_now()?;
    let (trusted, open) = {
      let mut session = self.session();
      session.record_activity();
      let sync = &session.unlocked()?.payload.sync;
      (sync.devices.clone(), invitation::open_secrets(sync, now))
    };
    let received = file::read_file(&bytes, &identity, &trusted, &open)?;

    let storage = self.storage()?;
    let mut session = self.session();
    session.commit(&storage, VaultStorage::save, |payload| {
      let pairing = match &received.invitation {
        Some(secret) if invitation::consume(&mut payload.sync, secret) => true,
        Some(_) => return Err(AppError::SyncFile(file::FileError::NoInvitation)),
        None if devices::is_trusted(&payload.sync.devices, &received.sender) => false,
        None => return Err(AppError::UntrustedDevice),
      };
      apply_snapshot(payload, received.sender, received.snapshot, pairing, now)
    })
  }
}

/// Merges a peer's snapshot into the vault: accounts, then
/// devices. When `pairing`, the peer joins the device set under the
/// name it gave. A device that learns it was removed forgets its
/// identity. The one place a merge is applied, whatever brought the
/// snapshot.
///
/// # Errors
///
/// [`AppError::Devices`] if the device sets cannot be merged; nothing is
/// changed then.
pub(crate) fn apply_snapshot(
  payload: &mut VaultPayload,
  peer: PublicKey,
  snapshot: Snapshot,
  pairing: bool,
  now: u64,
) -> Result<SyncSummary, AppError> {
  let (peer_name, remote, remote_devices) = snapshot.into_parts();
  let (merged, summary) = merge(&Replica::of(payload), &remote);
  let mut devices_merged = devices::merge(&payload.sync.devices, &remote_devices)?;
  if pairing {
    devices::add(
      &mut devices_merged,
      Member {
        public_key: peer,
        name: peer_name.clone(),
        added_at: now,
      },
    )?;
  }
  merged.store_in(payload);
  payload.sync.devices = devices_merged;
  identity::forget_if_removed(&mut payload.sync);
  Ok(SyncSummary {
    peer_name,
    added: summary.added,
    updated: summary.updated,
    deleted: summary.deleted,
    paired: pairing,
  })
}

/// This device's whole sync state, to send.
pub(crate) fn local_snapshot(payload: &VaultPayload, identity: &DeviceIdentity) -> Snapshot {
  Snapshot::new(
    identity.name.clone(),
    Replica::of(payload),
    payload.sync.devices.clone(),
  )
}

/// A device id from the frontend: a public key in hex.
fn parse_device_id(id: &str) -> Result<PublicKey, AppError> {
  let mut key = [0u8; PUBLIC_KEY_LEN];
  hex::decode_to_slice(id, &mut key).map_err(|_| AppError::InvalidDeviceId(id.to_owned()))?;
  Ok(PublicKey(key))
}

/// What this device is called until the user renames it.
/// On Linux, the machine's host name; on Android, whose host name is
/// always `localhost`, the phone's model (`Build.MODEL`), so that two
/// phones are told apart.
fn default_device_name() -> String {
  #[cfg(target_os = "android")]
  {
    android_system_properties::AndroidSystemProperties::new()
      .get("ro.product.model")
      .map(|model| model.trim().to_owned())
      .filter(|model| !model.is_empty())
      .unwrap_or_else(|| "Android phone".to_owned())
  }
  #[cfg(not(target_os = "android"))]
  {
    let host = rustix::system::uname();
    let host = host.nodename().to_string_lossy();
    if host.is_empty() {
      "Linux computer".to_owned()
    } else {
      host.into_owned()
    }
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use vault_sync::devices;

  use super::*;
  use crate::state::fixture::*;

  #[test]
  fn a_new_vault_has_no_paired_device() {
    let fixture = Fixture::unlocked();

    assert!(!fixture.state.has_paired_devices().unwrap());
  }

  #[test]
  fn the_first_sync_creates_an_identity_that_survives_a_restart() {
    let fixture = Fixture::unlocked();
    let identity = fixture.state.sync_identity().unwrap();

    let fixture = fixture.restart();
    fixture.state.unlock_vault(&secret(PASSWORD)).unwrap();

    assert_eq!(fixture.state.sync_identity().unwrap(), identity);
  }

  #[test]
  fn the_identity_is_named_after_the_device_and_listed_once_it_invites() {
    let fixture = Fixture::unlocked();
    let identity = fixture.state.sync_identity().unwrap();
    assert_eq!(identity.name, default_device_name());
    assert!(!identity.name.is_empty());

    fixture.state.create_invitation().unwrap();

    let session = fixture.state.session();
    let devices_here = &session.unlocked().unwrap().payload.sync.devices;
    assert!(devices::is_trusted(devices_here, &identity.public_key));
  }

  #[test]
  fn a_vault_copied_to_another_installation_gets_its_own_identity() {
    let original = Fixture::unlocked();
    let original_identity = original.state.sync_identity().unwrap();
    let copy = Fixture::new();
    let target = copy.paths().vault().to_path_buf();
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::copy(original.paths().vault(), &target).unwrap();

    copy.state.unlock_vault(&secret(PASSWORD)).unwrap();
    let copy_identity = copy.state.sync_identity().unwrap();

    assert_ne!(copy_identity.public_key, original_identity.public_key);
    let reopened = copy.restart();
    reopened.state.unlock_vault(&secret(PASSWORD)).unwrap();
    assert_eq!(
      reopened.state.sync_identity().unwrap(),
      copy_identity,
      "the renewed identity was saved"
    );
  }

  // ---- devices, invitations and files ---------------------------------

  use std::path::PathBuf;

  use crate::error::AppErrorKind;

  fn file_in(fixture: &Fixture, name: &str) -> PathBuf {
    fixture.dir.path().join(name)
  }

  fn device_id(fixture: &Fixture) -> String {
    fixture.state.sync_overview().unwrap().this_device.id
  }

  /// Pairs `joiner` with `inviter` the way two phones do: a join file
  /// one way, a sync file back.
  fn pair_by_files(inviter: &Fixture, joiner: &Fixture) {
    let (invitation, _) = inviter.state.create_invitation().unwrap();
    let join = file_in(joiner, "join.arsu");
    let qr = secret(&invitation.qr_text());
    joiner.state.export_join_file(&qr, &join).unwrap();
    inviter.state.import_sync_file(&join).unwrap();
    let reply = file_in(inviter, "reply.arsu");
    inviter
      .state
      .export_sync_file(&device_id(joiner), &reply)
      .unwrap();
    joiner.state.import_sync_file(&reply).unwrap();
  }

  #[test]
  fn the_overview_shows_this_device_alone_at_first() {
    let fixture = Fixture::unlocked();

    let overview = fixture.state.sync_overview().unwrap();

    assert_eq!(overview.this_device.name, default_device_name());
    assert_eq!(overview.this_device.id.len(), 64);
    assert_eq!(
      overview.this_device.fingerprint.len(),
      19,
      "XXXX XXXX XXXX XXXX"
    );
    assert!(overview.devices.is_empty());
  }

  #[test]
  fn an_invitation_is_kept_open_in_the_vault() {
    let fixture = Fixture::unlocked();

    let (invitation, expires_at) = fixture.state.create_invitation().unwrap();

    assert!(invitation.qr_text().starts_with("ARSU-INVITE:"));
    assert!(expires_at > 0);
    let session = fixture.state.session();
    assert_eq!(
      session.unlocked().unwrap().payload.sync.invitations.len(),
      1
    );
  }

  #[test]
  fn two_devices_pair_by_files_and_share_their_accounts() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    a.add_account("alice");
    b.add_account("bob");

    pair_by_files(&a, &b);

    assert_eq!(a.labels(), ["alice", "bob"]);
    assert_eq!(b.labels(), ["alice", "bob"]);
    let names = |fixture: &Fixture| -> Vec<String> {
      fixture
        .state
        .sync_overview()
        .unwrap()
        .devices
        .into_iter()
        .map(|device| device.id)
        .collect()
    };
    assert_eq!(names(&a), [device_id(&b)]);
    assert_eq!(names(&b), [device_id(&a)]);
  }

  #[test]
  fn importing_a_join_file_reports_the_pairing() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    b.add_account("bob");
    let (invitation, _) = a.state.create_invitation().unwrap();
    let join = file_in(&b, "join.arsu");
    b.state
      .export_join_file(&secret(&invitation.qr_text()), &join)
      .unwrap();

    let summary = a.state.import_sync_file(&join).unwrap();

    assert_eq!(
      summary,
      SyncSummary {
        peer_name: default_device_name(),
        added: 1,
        updated: 0,
        deleted: 0,
        paired: true,
      }
    );
    assert!(
      matches!(
        a.state.import_sync_file(&join).unwrap_err().kind(),
        AppErrorKind::NoMatchingInvitation
      ),
      "an invitation is used once"
    );
  }

  #[test]
  fn a_deletion_travels_in_the_next_sync_file() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    a.add_account("alice");
    pair_by_files(&a, &b);
    let id = a.state.list_entries().unwrap()[0].id.clone();
    a.state.delete_entry(id.parse().unwrap()).unwrap();

    let file = file_in(&a, "sync.arsu");
    a.state.export_sync_file(&device_id(&b), &file).unwrap();
    let summary = b.state.import_sync_file(&file).unwrap();

    assert!(b.labels().is_empty());
    assert_eq!(summary.deleted, 1);
    assert!(!summary.paired);
  }

  #[test]
  fn a_sync_file_from_an_unpaired_device_is_refused() {
    let (a, b, stranger) = (
      Fixture::unlocked(),
      Fixture::unlocked(),
      Fixture::unlocked(),
    );
    pair_by_files(&a, &b);
    pair_by_files(&stranger, &Fixture::unlocked());
    let file = file_in(&stranger, "sync.arsu");
    // The stranger cannot even address a device it does not trust.
    assert_eq!(
      stranger
        .state
        .export_sync_file(&device_id(&a), &file)
        .unwrap_err()
        .kind(),
      AppErrorKind::UntrustedDevice
    );
  }

  #[test]
  fn a_removed_device_can_no_longer_sync() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    pair_by_files(&a, &b);
    let file = file_in(&b, "sync.arsu");
    b.state.export_sync_file(&device_id(&a), &file).unwrap();

    a.state.remove_sync_device(&device_id(&b)).unwrap();

    assert!(a.state.sync_overview().unwrap().devices.is_empty());
    assert_eq!(
      a.state.import_sync_file(&file).unwrap_err().kind(),
      AppErrorKind::UntrustedDevice
    );
    assert_eq!(
      a.state
        .export_sync_file(&device_id(&b), &file_in(&a, "x"))
        .unwrap_err()
        .kind(),
      AppErrorKind::UntrustedDevice
    );
  }

  #[test]
  fn the_inviting_devices_own_name_replaces_the_placeholder() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());

    pair_by_files(&a, &b);

    let seen_by_b = b.state.sync_overview().unwrap().devices;
    assert_eq!(seen_by_b.len(), 1);
    assert_eq!(seen_by_b[0].name, default_device_name());
    assert_ne!(seen_by_b[0].added_at, PENDING_ADDED_AT);
  }

  #[test]
  fn a_device_waiting_for_its_inviters_reply_lists_it_as_pending() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    let (invitation, _) = a.state.create_invitation().unwrap();

    b.state
      .export_join_file(&secret(&invitation.qr_text()), &file_in(&b, "join.arsu"))
      .unwrap();

    let seen_by_b = b.state.sync_overview().unwrap().devices;
    assert_eq!(seen_by_b[0].name, INVITING_DEVICE_NAME);
    assert_eq!(seen_by_b[0].added_at, PENDING_ADDED_AT);
  }

  #[test]
  fn a_join_file_that_cannot_be_written_trusts_nobody() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    let (invitation, _) = a.state.create_invitation().unwrap();
    let nowhere = b.dir.path().join("missing/dir/join.arsu");

    assert_eq!(
      b.state
        .export_join_file(&secret(&invitation.qr_text()), &nowhere)
        .unwrap_err()
        .kind(),
      AppErrorKind::FileWrite
    );
    assert!(b.state.sync_overview().unwrap().devices.is_empty());
  }

  #[test]
  fn recipients_and_invitations_are_checked_before_any_dialog() {
    let (a, b) = (Fixture::unlocked(), Fixture::unlocked());
    assert_eq!(
      a.state
        .check_sync_recipient(&device_id(&b))
        .unwrap_err()
        .kind(),
      AppErrorKind::UntrustedDevice
    );
    assert_eq!(
      a.state
        .check_join_invitation(&secret("not an invitation"))
        .unwrap_err()
        .kind(),
      AppErrorKind::NotAnInvitation
    );

    pair_by_files(&a, &b);
    let (invitation, _) = b.state.create_invitation().unwrap();

    assert!(a.state.check_sync_recipient(&device_id(&b)).is_ok());
    assert!(
      a.state
        .check_join_invitation(&secret(&invitation.qr_text()))
        .is_ok()
    );
    a.state.lock_vault();
    assert_eq!(
      a.state
        .check_sync_recipient(&device_id(&b))
        .unwrap_err()
        .kind(),
      AppErrorKind::Locked
    );
  }

  #[test]
  fn a_device_not_in_the_vault_cannot_be_removed() {
    let (a, stranger) = (Fixture::unlocked(), Fixture::unlocked());

    assert_eq!(
      a.state
        .remove_sync_device(&device_id(&stranger))
        .unwrap_err()
        .kind(),
      AppErrorKind::UnknownDevice
    );
  }

  #[test]
  fn a_device_cannot_remove_itself() {
    let fixture = Fixture::unlocked();

    assert_eq!(
      fixture
        .state
        .remove_sync_device(&device_id(&fixture))
        .unwrap_err()
        .kind(),
      AppErrorKind::CannotRemoveThisDevice
    );
  }

  #[test]
  fn a_device_id_must_be_a_key_in_hex() {
    let fixture = Fixture::unlocked();

    for id in ["", "zz", &"a".repeat(63), &"g".repeat(64)] {
      assert_eq!(
        fixture.state.remove_sync_device(id).unwrap_err().kind(),
        AppErrorKind::InvalidDeviceId,
        "{id:?}"
      );
    }
  }

  #[test]
  fn text_that_is_not_an_invitation_is_refused() {
    let fixture = Fixture::unlocked();

    assert_eq!(
      fixture
        .state
        .export_join_file(
          &secret("otpauth://totp/x?secret=AB"),
          &file_in(&fixture, "j")
        )
        .unwrap_err()
        .kind(),
      AppErrorKind::NotAnInvitation
    );
  }

  #[test]
  fn a_failed_import_changes_nothing() {
    let fixture = Fixture::unlocked();
    fixture.add_account("alice");
    let garbage = file_in(&fixture, "garbage.arsu");
    fs::write(&garbage, b"not a sync file").unwrap();

    assert_eq!(
      fixture.state.import_sync_file(&garbage).unwrap_err().kind(),
      AppErrorKind::NotASyncFile
    );
    assert_eq!(fixture.labels(), ["alice"]);
  }

  #[test]
  fn a_locked_vault_has_no_identity_to_give() {
    let fixture = Fixture::new();

    assert!(matches!(
      fixture.state.sync_identity(),
      Err(AppError::Locked)
    ));
  }
}
