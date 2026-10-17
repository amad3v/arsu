//! A live sync or pairing session, without I/O.
//!
//! A [`Session`] is fed the bytes that arrived from the link and
//! returns what to do: bytes to send, the peer's snapshot to merge, or
//! the end of the session. It never touches the link, the vault or a
//! clock, so the whole protocol runs in tests between two sessions in
//! memory.
//!
//! ```text
//! initiator                                responder
//!   ── XX message 1 ───────────────────────▶
//!   ◀─────────────────────── XX message 2 ──  responder's key: checked
//!   ── XX message 3 ───────────────────────▶  initiator's key: checked
//!   ── Snapshot ───────────────────────────▶
//!   ◀─────────────────────────── Snapshot ──
//!        both: Output::Received → merge, save → Session::acknowledge
//!   ── Done ───────────────────────────────▶
//!   ◀─────────────────────────────── Done ──  both: Output::Finished
//! ```
//!
//! The initiator checks the responder's key before sending message 3,
//! so it never reveals its own identity to a device it does not trust.

use std::mem;

use snow::{HandshakeState, TransportState};
use vault_core::{DeviceIdentity, DeviceSet, PublicKey};
use zeroize::Zeroizing;

use crate::{
  devices,
  noise::{self, PairingSecret, Purpose},
  protocol::{Message, ProtocolError, Snapshot},
  wire::{self, FrameReader, MAX_NOISE_MESSAGE, Reassembler, WireError},
};

/// What the session is for, and which side this device is.
#[derive(Debug, Clone)]
pub enum Mode {
  /// Sync between paired devices. The peer must be trusted by the
  /// device set in this device's snapshot.
  Sync { initiator: bool },
  /// Pairing, on the device that showed the invitation (the responder:
  /// the BLE peripheral).
  Invite { secret: PairingSecret },
  /// Pairing, on the device that read the invitation (the initiator).
  /// `inviter` is the inviting device's key when the invitation carried
  /// it (the QR code does, the typed code does not).
  Join {
    secret: PairingSecret,
    inviter: Option<PublicKey>,
  },
}

/// What the caller must do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
  /// Send these bytes to the peer, in order.
  Send(Vec<u8>),
  /// The peer's whole sync state arrived: merge it, save the vault, then
  /// call [`Session::acknowledge`]. When pairing, `peer` is the device
  /// to add to the device set.
  Received {
    peer: PublicKey,
    snapshot: Box<Snapshot>,
  },
  /// Both sides have merged. The link can be closed.
  Finished,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
  #[error("the secure connection failed: wrong pairing code, or not an Arsu device")]
  Noise(#[from] snow::Error),
  #[error(
    "the other device ({}) is not paired with your vault yet: pair it, or, if another of your \
     devices paired it, sync with that one first",
    .0.fingerprint()
  )]
  UntrustedPeer(PublicKey),
  #[error("this device ({}) is not the one that showed the invitation", .0.fingerprint())]
  UnexpectedPeer(PublicKey),
  #[error("the other device uses this device's own identity")]
  OwnKey,
  #[error(transparent)]
  Wire(#[from] WireError),
  #[error(transparent)]
  Protocol(#[from] ProtocolError),
  #[error("the other device broke the sync protocol")]
  UnexpectedMessage,
  #[error("the session has already ended")]
  Closed,
}

/// A sync or pairing session. See the module documentation.
pub struct Session {
  mode: Mode,
  identity_key: PublicKey,
  /// The devices this one trusts, to check a sync peer against.
  trusted: DeviceSet,
  /// This device's snapshot, until it is sent.
  snapshot: Option<Snapshot>,
  state: State,
  frames: FrameReader,
  reassembler: Reassembler,
  peer: Option<PublicKey>,
  received: bool,
  acknowledged: bool,
  peer_done: bool,
}

enum State {
  Handshake(Box<HandshakeState>),
  Transport(Box<TransportState>),
  Closed,
}

impl Session {
  /// Starts a session. `snapshot` is this device's sync state, sent once
  /// the peer is authenticated. An initiator's first message is in the
  /// returned outputs.
  ///
  /// # Errors
  ///
  /// [`SessionError::Noise`] if the handshake cannot be set up (an
  /// invalid private key, a failing random generator).
  pub fn new(
    mode: Mode,
    identity: &DeviceIdentity,
    snapshot: Snapshot,
  ) -> Result<(Self, Vec<Output>), SessionError> {
    let (purpose, initiator, secret) = match &mode {
      Mode::Sync { initiator } => (Purpose::Session, *initiator, None),
      Mode::Invite { secret } => (Purpose::Pairing, false, Some(secret)),
      Mode::Join { secret, .. } => (Purpose::Pairing, true, Some(secret)),
    };
    let handshake = noise::handshake(purpose, initiator, identity, secret, None)?;
    let mut session = Self {
      identity_key: identity.public_key,
      trusted: snapshot.devices.clone(),
      snapshot: Some(snapshot),
      state: State::Handshake(Box::new(handshake)),
      frames: FrameReader::default(),
      reassembler: Reassembler::default(),
      peer: None,
      received: false,
      acknowledged: false,
      peer_done: false,
      mode,
    };
    let mut outputs = Vec::new();
    session.guard(|session| session.advance_handshake(&mut outputs))?;
    Ok((session, outputs))
  }

  /// Takes bytes from the link.
  ///
  /// # Errors
  ///
  /// Any [`SessionError`]; the session is then closed, and every later
  /// call fails with [`SessionError::Closed`].
  pub fn receive(&mut self, bytes: &[u8]) -> Result<Vec<Output>, SessionError> {
    self.guard(|session| {
      let mut outputs = Vec::new();
      for frame in session.frames.push(bytes)? {
        session.handle_frame(&frame, &mut outputs)?;
      }
      Ok(outputs)
    })
  }

  /// Confirms that the peer's snapshot was merged and saved.
  ///
  /// # Errors
  ///
  /// [`SessionError::UnexpectedMessage`] if no snapshot was received or
  /// it was already acknowledged; [`SessionError::Closed`] after an
  /// error.
  pub fn acknowledge(&mut self) -> Result<Vec<Output>, SessionError> {
    self.guard(|session| {
      if !session.received || session.acknowledged {
        return Err(SessionError::UnexpectedMessage);
      }
      session.acknowledged = true;
      let mut outputs = Vec::new();
      session.send(&Message::Done, &mut outputs)?;
      session.finish_if_done(&mut outputs);
      Ok(outputs)
    })
  }

  /// The authenticated peer, once the handshake has identified it.
  #[must_use]
  pub fn peer(&self) -> Option<PublicKey> {
    self.peer
  }

  /// Runs `step`, closing the session if it fails.
  fn guard<T>(
    &mut self,
    step: impl FnOnce(&mut Self) -> Result<T, SessionError>,
  ) -> Result<T, SessionError> {
    if matches!(self.state, State::Closed) {
      return Err(SessionError::Closed);
    }
    let result = step(self);
    if result.is_err() {
      self.state = State::Closed;
    }
    result
  }

  fn handle_frame(&mut self, frame: &[u8], outputs: &mut Vec<Output>) -> Result<(), SessionError> {
    // Handshake payloads are empty, and transport payloads fit a frame.
    let mut plaintext = Zeroizing::new(vec![0u8; MAX_NOISE_MESSAGE]);
    match &mut self.state {
      State::Handshake(handshake) => {
        handshake.read_message(frame, &mut plaintext)?;
        self.advance_handshake(outputs)
      }
      State::Transport(transport) => {
        let length = transport.read_message(frame, &mut plaintext)?;
        match self.reassembler.push(&plaintext[..length])? {
          Some(message) => self.handle_message(&message, outputs),
          None => Ok(()),
        }
      }
      State::Closed => Err(SessionError::Closed),
    }
  }

  /// After each handshake message: identify the peer as soon as its key
  /// is known, write our next message if it is our turn, and switch to
  /// transport mode once the handshake is complete.
  fn advance_handshake(&mut self, outputs: &mut Vec<Output>) -> Result<(), SessionError> {
    let State::Handshake(handshake) = &mut self.state else {
      return Err(SessionError::UnexpectedMessage);
    };
    let remote = handshake.get_remote_static().and_then(noise::public_key);
    if self.peer.is_none()
      && let Some(peer) = remote
    {
      self.check_peer(peer)?;
      self.peer = Some(peer);
    }

    let State::Handshake(handshake) = &mut self.state else {
      return Err(SessionError::UnexpectedMessage);
    };
    if !handshake.is_handshake_finished() && handshake.is_my_turn() {
      let mut message = vec![0u8; MAX_NOISE_MESSAGE];
      let length = handshake.write_message(&[], &mut message)?;
      outputs.push(Output::Send(wire::frame(&message[..length])?));
    }
    if handshake.is_handshake_finished() {
      self.start_transport(outputs)?;
    }
    Ok(())
  }

  /// Whether the handshake's peer may be talked to.
  fn check_peer(&self, peer: PublicKey) -> Result<(), SessionError> {
    if peer == self.identity_key {
      return Err(SessionError::OwnKey);
    }
    match &self.mode {
      Mode::Sync { .. } if !devices::is_trusted(&self.trusted, &peer) => {
        Err(SessionError::UntrustedPeer(peer))
      }
      Mode::Join {
        inviter: Some(inviter),
        ..
      } if *inviter != peer => Err(SessionError::UnexpectedPeer(peer)),
      _ => Ok(()),
    }
  }

  fn start_transport(&mut self, outputs: &mut Vec<Output>) -> Result<(), SessionError> {
    let State::Handshake(handshake) = mem::replace(&mut self.state, State::Closed) else {
      return Err(SessionError::UnexpectedMessage);
    };
    self.state = State::Transport(Box::new(handshake.into_transport_mode()?));
    let snapshot = self
      .snapshot
      .take()
      .ok_or(SessionError::UnexpectedMessage)?;
    self.send(&Message::Snapshot(Box::new(snapshot)), outputs)
  }

  fn handle_message(
    &mut self,
    bytes: &[u8],
    outputs: &mut Vec<Output>,
  ) -> Result<(), SessionError> {
    match Message::decode(bytes)? {
      Message::Snapshot(snapshot) => {
        let peer = self.peer.ok_or(SessionError::UnexpectedMessage)?;
        if self.received {
          return Err(SessionError::UnexpectedMessage);
        }
        self.received = true;
        outputs.push(Output::Received { peer, snapshot });
      }
      Message::Done => {
        // The peer answers Done only after our snapshot, and sends its
        // own snapshot first.
        if !self.received || self.peer_done {
          return Err(SessionError::UnexpectedMessage);
        }
        self.peer_done = true;
        self.finish_if_done(outputs);
      }
    }
    Ok(())
  }

  /// Ends the session once both sides have merged.
  fn finish_if_done(&mut self, outputs: &mut Vec<Output>) {
    if self.acknowledged && self.peer_done {
      self.state = State::Closed;
      outputs.push(Output::Finished);
    }
  }

  /// Encrypts and frames an application message, as one [`Output::Send`].
  fn send(&mut self, message: &Message, outputs: &mut Vec<Output>) -> Result<(), SessionError> {
    let State::Transport(transport) = &mut self.state else {
      return Err(SessionError::UnexpectedMessage);
    };
    let mut bytes = Vec::new();
    let mut ciphertext = vec![0u8; MAX_NOISE_MESSAGE];
    for chunk in wire::split(&message.encode()?)? {
      let length = transport.write_message(&chunk, &mut ciphertext)?;
      bytes.extend(wire::frame(&ciphertext[..length])?);
    }
    outputs.push(Output::Send(bytes));
    Ok(())
  }
}

#[cfg(test)]
mod tests;
