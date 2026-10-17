//! Live sync and pairing sessions, over any link that moves GATT chunks.
//!
//! [`AppState::run_live_session`] runs one session to its end: it builds
//! this device's snapshot, drives a [`GattLink`] with the chunks the
//! [`ChunkPeer`] delivers, merges the peer's snapshot through
//! [`apply_snapshot`] and saves the vault. It is the same for both
//! Bluetooth roles and every platform; only the [`ChunkPeer`] differs —
//! `BlueZ`'s peripheral on Linux, `tauri-plugin-blec`'s central on Android,
//! channels in the tests.

use std::collections::VecDeque;

use storage::VaultStorage;
use vault_core::{DeviceIdentity, PublicKey};
use vault_sync::{
  devices,
  gatt::{GattLink, LinkError, LinkOutput},
  identity,
  invitation::{self, Invitation},
  noise::PairingSecret,
  protocol::Snapshot,
  session::{Mode, SessionError},
};

use super::{
  AppState,
  sync::{apply_snapshot, local_snapshot},
};
use crate::{clock::unix_now, dto::SyncSummary, error::AppError};

/// What a live session is for, and which side this device is.
#[derive(Debug, Clone)]
pub enum LiveRole {
  /// Sync with a paired device, as the BLE peripheral.
  SyncResponder,
  /// Sync with a paired device, as the BLE central.
  SyncInitiator,
  /// Pairing, on the device that showed `invitation`: the peripheral.
  Invite(Invitation),
  /// Pairing, on the device that read an invitation: the central. The
  /// inviting device's key is known when the invitation was a QR code.
  Join {
    secret: PairingSecret,
    inviter: Option<PublicKey>,
  },
}

/// The roles as the diagnostic log names them ([`LiveRole::label`]).
pub mod labels {
  pub const SYNC_WAITING: &str = "sync-waiting";
  pub const SYNC_CONNECTING: &str = "sync-connecting";
  pub const PAIRING_INVITING: &str = "pairing-inviting";
  pub const PAIRING_JOINING: &str = "pairing-joining";
}

impl LiveRole {
  /// What the role is, for the diagnostic log: never the invitation.
  #[must_use]
  pub fn label(&self) -> &'static str {
    match self {
      Self::SyncResponder => labels::SYNC_WAITING,
      Self::SyncInitiator => labels::SYNC_CONNECTING,
      Self::Invite(_) => labels::PAIRING_INVITING,
      Self::Join { .. } => labels::PAIRING_JOINING,
    }
  }

  fn initiates(&self) -> bool {
    matches!(self, Self::SyncInitiator | Self::Join { .. })
  }

  fn pairs(&self) -> bool {
    matches!(self, Self::Invite(_) | Self::Join { .. })
  }

  fn mode(&self) -> Mode {
    match self {
      Self::SyncResponder => Mode::Sync { initiator: false },
      Self::SyncInitiator => Mode::Sync { initiator: true },
      Self::Invite(invitation) => Mode::Invite {
        secret: invitation.secret.clone(),
      },
      Self::Join { secret, inviter } => Mode::Join {
        secret: secret.clone(),
        inviter: *inviter,
      },
    }
  }
}

/// A link that moves GATT chunks to and from the peer.
pub trait ChunkPeer {
  /// The next chunk from the peer, with the link's ATT MTU; `None` once
  /// the peer is gone.
  async fn next_chunk(&mut self) -> Option<(Vec<u8>, usize)>;

  /// Sends a chunk to the peer.
  async fn send_chunk(&mut self, chunk: Vec<u8>) -> Result<(), AppError>;
}

impl AppState {
  /// Runs one live session over `peer` to its end, merging and saving
  /// the peer's snapshot. As the initiator, `att_mtu` is the link's MTU;
  /// as the responder, the MTU of the first chunk received is used.
  ///
  /// # Errors
  ///
  /// [`AppError::Link`] if the session fails, [`AppError::Disconnected`]
  /// if the peer goes before it ends, or the errors of preparing and
  /// applying a snapshot (see [`Self::import_sync_file`]).
  pub(crate) async fn run_live_session(
    &self,
    role: LiveRole,
    peer: &mut impl ChunkPeer,
    att_mtu: usize,
  ) -> Result<SyncSummary, AppError> {
    log::info!(role = role.label(); "session started");
    let outcome = self.live_session(&role, peer, att_mtu).await;
    // A failure is the caller's to record, with what it was doing.
    if let Ok(summary) = &outcome {
      log::info!(
        role = role.label(),
        added = summary.added,
        updated = summary.updated,
        deleted = summary.deleted;
        "session finished"
      );
    }
    outcome
  }

  async fn live_session(
    &self,
    role: &LiveRole,
    peer: &mut impl ChunkPeer,
    att_mtu: usize,
  ) -> Result<SyncSummary, AppError> {
    let (mut link, pending) = if role.initiates() {
      let (identity, snapshot) = self.live_snapshot(role)?;
      GattLink::new(role.mode(), &identity, snapshot, att_mtu)?
    } else {
      // The responder learns the MTU from the central's first write, and
      // decrypts its snapshot only then: the wait can be long, and the
      // vault may lock or the invitation expire meanwhile.
      let (chunk, mtu) = peer.next_chunk().await.ok_or(AppError::Disconnected)?;
      let (identity, snapshot) = self.live_snapshot(role)?;
      let (mut link, mut outputs) = GattLink::new(role.mode(), &identity, snapshot, mtu)?;
      outputs.extend(link.receive(&chunk)?);
      (link, outputs)
    };

    let mut pending: VecDeque<LinkOutput> = pending.into();
    let mut summary = None;
    loop {
      // Everything the link asked for, including what acknowledging adds,
      // before waiting for the peer again.
      while let Some(output) = pending.pop_front() {
        match output {
          LinkOutput::Chunk(chunk) => peer.send_chunk(chunk).await?,
          LinkOutput::Received {
            peer: key,
            snapshot,
          } => {
            log::info!(role = role.label(), peer:% = key.fingerprint(); "snapshot received");
            summary = Some(self.live_merge(role, key, *snapshot)?);
            pending.extend(link.acknowledge()?);
          }
          LinkOutput::Finished => {
            return summary.ok_or(AppError::Link(LinkError::Session(
              SessionError::UnexpectedMessage,
            )));
          }
        }
      }
      let (chunk, _) = peer.next_chunk().await.ok_or(AppError::Disconnected)?;
      pending.extend(link.receive(&chunk)?);
    }
  }

  /// This device's identity and snapshot for a session. Pairing lists
  /// this device in its own device set first, so the peer learns its
  /// name; inviting, only while the invitation is still open.
  fn live_snapshot(&self, role: &LiveRole) -> Result<(DeviceIdentity, Snapshot), AppError> {
    let identity = self.sync_identity()?;
    let now = unix_now()?;
    if let LiveRole::Invite(invitation) = role {
      let open = {
        let session = self.session();
        invitation::open_secrets(&session.unlocked()?.payload.sync, now)
      };
      if !open.iter().any(|secret| secret == &invitation.secret) {
        return Err(AppError::SyncFile(
          vault_sync::file::FileError::NoInvitation,
        ));
      }
    }
    let storage = self.storage()?;
    let mut session = self.session();
    session.record_activity();
    if role.pairs() {
      session.commit(&storage, VaultStorage::save_without_rotation, |payload| {
        identity::list_self(&mut payload.sync, now)?;
        Ok(local_snapshot(payload, &identity))
      })
    } else {
      Ok(local_snapshot(&session.unlocked()?.payload, &identity))
    }
    .map(|snapshot| {
      log::info!(
        role = role.label(),
        device:% = identity.public_key.fingerprint(),
        trusted_devices = devices::trusted(&snapshot.devices).count();
        "snapshot ready"
      );
      (identity, snapshot)
    })
  }

  /// Merges the peer's snapshot and saves the vault. The session already
  /// checked the peer; this checks again against the vault as it is now:
  /// a sync peer must still be trusted, and an invitation still open.
  fn live_merge(
    &self,
    role: &LiveRole,
    peer: PublicKey,
    snapshot: Snapshot,
  ) -> Result<SyncSummary, AppError> {
    let now = unix_now()?;
    let storage = self.storage()?;
    let mut session = self.session();
    session.record_activity();
    session.commit(&storage, VaultStorage::save, |payload| {
      match role {
        LiveRole::Invite(invitation) => {
          if !invitation::consume(&mut payload.sync, &invitation.secret, now) {
            return Err(AppError::SyncFile(
              vault_sync::file::FileError::NoInvitation,
            ));
          }
        }
        LiveRole::Join { .. } => {}
        LiveRole::SyncResponder | LiveRole::SyncInitiator => {
          if !devices::is_trusted(&payload.sync.devices, &peer) {
            return Err(AppError::UntrustedDevice);
          }
        }
      }
      apply_snapshot(payload, peer, snapshot, role.pairs(), now)
    })
  }
}

#[cfg(test)]
mod tests {
  use std::fs;

  use super::*;
  use crate::{
    error::AppErrorKind,
    state::{
      fixture::{Fixture, PASSWORD, secret},
      wire::{pair, side, wires},
    },
  };

  #[tokio::test]
  async fn a_phone_pairs_with_a_computer_over_bluetooth_and_both_share_accounts() {
    let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
    computer.add_account("alice");
    phone.add_account("bob");

    let (computer_summary, phone_summary) = pair(&computer, &phone).await;

    assert!(computer_summary.paired && phone_summary.paired);
    assert_eq!(computer_summary.added, 1);
    assert_eq!(phone_summary.added, 1);
    assert_eq!(computer.labels(), ["alice", "bob"]);
    assert_eq!(phone.labels(), ["alice", "bob"]);
    assert_eq!(computer.other_devices(), 1);
    assert_eq!(phone.other_devices(), 1);
    assert!(computer.state.has_paired_devices().unwrap());
    assert!(phone.state.has_paired_devices().unwrap());
  }

  #[tokio::test]
  async fn pairing_uses_the_invitation_up() {
    let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());

    pair(&computer, &phone).await;

    let session = computer.state.session();
    assert!(
      session
        .unlocked()
        .unwrap()
        .payload
        .sync
        .invitations
        .is_empty()
    );
  }

  #[tokio::test]
  async fn paired_devices_sync_over_bluetooth() {
    let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
    pair(&computer, &phone).await;
    phone.add_account("carol");

    let (central, peripheral) = wires(517);
    let (phone_summary, computer_summary) = tokio::join!(
      side(&phone, LiveRole::SyncInitiator, central, 517),
      side(&computer, LiveRole::SyncResponder, peripheral, 0),
    );

    assert_eq!(computer_summary.unwrap().added, 1);
    assert!(!phone_summary.unwrap().paired);
    assert_eq!(computer.labels(), ["carol"]);
  }

  #[tokio::test]
  async fn paired_devices_sync_after_both_apps_restart() {
    let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
    computer.add_account("alice");
    pair(&computer, &phone).await;
    let (computer, phone) = (computer.restart(), phone.restart());
    computer.state.unlock_vault(&secret(PASSWORD)).unwrap();
    phone.state.unlock_vault(&secret(PASSWORD)).unwrap();
    phone.add_account("carol");

    let (central, peripheral) = wires(517);
    let (phone_summary, computer_summary) = tokio::join!(
      side(&phone, LiveRole::SyncInitiator, central, 517),
      side(&computer, LiveRole::SyncResponder, peripheral, 0),
    );

    assert_eq!(computer_summary.unwrap().added, 1);
    assert!(phone_summary.is_ok());
    assert_eq!(computer.labels(), ["alice", "carol"]);
  }

  #[tokio::test]
  async fn paired_devices_sync_after_locking_and_unlocking() {
    let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
    pair(&computer, &phone).await;
    for fixture in [&computer, &phone] {
      fixture.state.lock_vault();
      fixture.state.unlock_vault(&secret(PASSWORD)).unwrap();
    }
    phone.add_account("carol");

    let (central, peripheral) = wires(517);
    let (phone_summary, computer_summary) = tokio::join!(
      side(&phone, LiveRole::SyncInitiator, central, 517),
      side(&computer, LiveRole::SyncResponder, peripheral, 0),
    );

    assert!(phone_summary.is_ok(), "{phone_summary:?}");
    assert_eq!(computer_summary.unwrap().added, 1);
  }

  #[tokio::test]
  async fn an_unpaired_phone_cannot_sync() {
    let (computer, stranger) = (Fixture::unlocked(), Fixture::unlocked());
    computer.add_account("secret");

    let (central, peripheral) = wires(185);
    let (stranger_result, computer_result) = tokio::join!(
      side(&stranger, LiveRole::SyncInitiator, central, 185),
      side(&computer, LiveRole::SyncResponder, peripheral, 0),
    );

    assert!(stranger_result.is_err());
    assert!(computer_result.is_err());
    assert!(stranger.labels().is_empty(), "nothing leaked");
  }

  #[tokio::test]
  async fn a_peer_that_disconnects_mid_session_changes_nothing() {
    let computer = Fixture::unlocked();
    computer.add_account("alice");
    let (central, peripheral) = wires(185);
    drop(central);

    let result = side(&computer, LiveRole::SyncResponder, peripheral, 0).await;

    assert_eq!(
      result.unwrap_err().kind(),
      AppErrorKind::BluetoothDisconnected
    );
    assert_eq!(computer.labels(), ["alice"]);
  }

  #[tokio::test]
  async fn a_wrong_pairing_code_pairs_nobody() {
    let (computer, phone) = (Fixture::unlocked(), Fixture::unlocked());
    let (invitation, _) = computer.state.create_invitation().unwrap();
    let wrong = LiveRole::Join {
      secret: PairingSecret::from_bytes([0; 32]),
      inviter: None,
    };

    let (central, peripheral) = wires(185);
    let (phone_result, computer_result) = tokio::join!(
      side(&phone, wrong, central, 185),
      side(&computer, LiveRole::Invite(invitation), peripheral, 0),
    );

    assert!(phone_result.is_err() && computer_result.is_err());
    assert_eq!(computer.other_devices(), 0);
    assert_eq!(phone.other_devices(), 0);
    let _ = fs::metadata(computer.paths().vault()).unwrap();
  }
}
