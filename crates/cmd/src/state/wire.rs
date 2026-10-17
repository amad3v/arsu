//! In-memory Bluetooth links for tests: two app states run live sessions
//! against each other through channels.

use tokio::sync::mpsc;

use super::{ChunkPeer, LiveRole, fixture::Fixture};
use crate::{dto::SyncSummary, error::AppError};

/// One end of an in-memory Bluetooth link.
pub struct Wire {
  incoming: mpsc::UnboundedReceiver<Vec<u8>>,
  outgoing: mpsc::UnboundedSender<Vec<u8>>,
  mtu: usize,
}

impl ChunkPeer for Wire {
  async fn next_chunk(&mut self) -> Option<(Vec<u8>, usize)> {
    self.incoming.recv().await.map(|chunk| (chunk, self.mtu))
  }

  fn send_chunk(&mut self, chunk: Vec<u8>) -> impl Future<Output = Result<(), AppError>> {
    assert!(chunk.len() <= self.mtu - 3, "a chunk fits the MTU");
    std::future::ready(
      self
        .outgoing
        .send(chunk)
        .map_err(|_| AppError::Disconnected),
    )
  }
}

pub fn wires(mtu: usize) -> (Wire, Wire) {
  let (to_b, from_a) = mpsc::unbounded_channel();
  let (to_a, from_b) = mpsc::unbounded_channel();
  (
    Wire {
      incoming: from_b,
      outgoing: to_b,
      mtu,
    },
    Wire {
      incoming: from_a,
      outgoing: to_a,
      mtu,
    },
  )
}

/// Runs one side of a session. The wire is dropped when the session
/// returns, which the other side sees as a disconnection — as it would
/// over Bluetooth.
pub async fn side(
  fixture: &Fixture,
  role: LiveRole,
  mut wire: Wire,
  mtu: usize,
) -> Result<SyncSummary, AppError> {
  fixture.state.run_live_session(role, &mut wire, mtu).await
}

/// Pairs two devices over Bluetooth: `computer` invites, `phone` joins
/// from the QR code.
pub async fn pair(computer: &Fixture, phone: &Fixture) -> (SyncSummary, SyncSummary) {
  let (invitation, _) = computer.state.create_invitation().unwrap();
  let joined = LiveRole::Join {
    secret: invitation.secret.clone(),
    inviter: Some(invitation.inviter),
  };
  let (central, peripheral) = wires(185);
  let (phone_summary, computer_summary) = tokio::join!(
    side(phone, joined, central, 185),
    side(computer, LiveRole::Invite(invitation), peripheral, 0),
  );
  (computer_summary.unwrap(), phone_summary.unwrap())
}
