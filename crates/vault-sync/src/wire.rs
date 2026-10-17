//! Bytes on the link.
//!
//! Two layers, both plain byte shuffling, no cryptography:
//!
//! * **Frames** carry Noise messages over a byte stream: each Noise
//!   message (at most 65 535 bytes, Noise spec §3) is preceded by its
//!   length as a 16-bit big-endian integer, as the spec recommends
//!   (§13). [`FrameReader`] cuts an incoming stream back into messages,
//!   however the link happened to split it.
//! * **Chunks** carry application messages, which may be larger than one
//!   Noise message: [`split`] prefixes the message with its length as a
//!   32-bit big-endian integer and cuts it into pieces that each fit in
//!   one Noise transport message; [`Reassembler`] puts them back
//!   together.
//!
//! Plaintext buffers are zeroized when dropped: application messages
//! carry seeds.

use zeroize::Zeroizing;

/// Longest Noise message (Noise spec §3).
pub const MAX_NOISE_MESSAGE: usize = 65_535;

/// Length of the Poly1305 tag that ends every Noise transport message.
pub const TAG_LEN: usize = 16;

/// Most plaintext one Noise transport message can carry.
pub const MAX_CHUNK: usize = MAX_NOISE_MESSAGE - TAG_LEN;

/// Longest application message accepted: a vault of a
/// thousand accounts is about 200 KB.
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;

/// Length of the prefix in front of each frame.
const FRAME_HEADER_LEN: usize = 2;

/// Length of the prefix in front of each application message.
const MESSAGE_HEADER_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
  #[error("a Noise message cannot be empty or longer than {MAX_NOISE_MESSAGE} bytes")]
  BadFrameLength,
  #[error("message of {0} bytes is longer than the {MAX_MESSAGE} bytes allowed")]
  MessageTooLarge(usize),
  #[error("a chunk carried more bytes than its message announced")]
  TrailingBytes,
}

/// `message` with its 16-bit length in front.
///
/// # Errors
///
/// [`WireError::BadFrameLength`] if `message` is empty or longer than a
/// Noise message can be.
pub fn frame(message: &[u8]) -> Result<Vec<u8>, WireError> {
  let length = u16::try_from(message.len())
    .ok()
    .filter(|&length| length > 0)
    .ok_or(WireError::BadFrameLength)?;
  let mut out = Vec::with_capacity(FRAME_HEADER_LEN + message.len());
  out.extend_from_slice(&length.to_be_bytes());
  out.extend_from_slice(message);
  Ok(out)
}

/// Cuts a byte stream back into the Noise messages it carries.
#[derive(Debug, Default)]
pub struct FrameReader {
  buffer: Vec<u8>,
}

impl FrameReader {
  /// Takes the next bytes from the link and returns every message they
  /// complete, in order. Bytes of an incomplete message are kept for
  /// the next call.
  ///
  /// # Errors
  ///
  /// [`WireError::BadFrameLength`] for a frame announcing an empty
  /// message. The stream cannot be resynchronized after an error.
  pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, WireError> {
    self.buffer.extend_from_slice(bytes);
    let mut messages = Vec::new();
    let mut start = 0;
    while let Some(header) = self.buffer.get(start..start + FRAME_HEADER_LEN) {
      let length = usize::from(u16::from_be_bytes([header[0], header[1]]));
      if length == 0 {
        return Err(WireError::BadFrameLength);
      }
      let body = start + FRAME_HEADER_LEN;
      let Some(message) = self.buffer.get(body..body + length) else {
        break;
      };
      messages.push(message.to_vec());
      start = body + length;
    }
    self.buffer.drain(..start);
    Ok(messages)
  }

  /// Whether no bytes of an incomplete message are waiting.
  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.buffer.is_empty()
  }
}

/// `message` with its 32-bit length in front, cut into chunks of at most
/// [`MAX_CHUNK`] bytes.
///
/// # Errors
///
/// [`WireError::MessageTooLarge`] past [`MAX_MESSAGE`].
pub fn split(message: &[u8]) -> Result<Vec<Zeroizing<Vec<u8>>>, WireError> {
  let length = checked_length(message.len())?;
  // The first chunk carries the header and as much of the message as
  // fits beside it; the others are slices of the message. Each chunk is
  // allocated once, at its final size.
  let (head, rest) = message.split_at(message.len().min(MAX_CHUNK - MESSAGE_HEADER_LEN));
  let mut first = Zeroizing::new(Vec::with_capacity(MESSAGE_HEADER_LEN + head.len()));
  first.extend_from_slice(&length.to_be_bytes());
  first.extend_from_slice(head);
  let mut chunks = Vec::with_capacity(1 + rest.len().div_ceil(MAX_CHUNK));
  chunks.push(first);
  chunks.extend(
    rest
      .chunks(MAX_CHUNK)
      .map(|chunk| Zeroizing::new(chunk.to_vec())),
  );
  Ok(chunks)
}

/// The length as it goes in a message header, if it is within limits.
fn checked_length(length: usize) -> Result<u32, WireError> {
  u32::try_from(length)
    .ok()
    .filter(|_| length <= MAX_MESSAGE)
    .ok_or(WireError::MessageTooLarge(length))
}

/// Puts chunks back together into application messages.
///
/// The message buffer is allocated once, at the size its header
/// announces, so it never grows: a growing buffer would leave copies of
/// the plaintext in every allocation it outgrew.
#[derive(Debug, Default)]
pub struct Reassembler {
  /// The header of the message in progress, as far as it has arrived.
  header: Vec<u8>,
  /// The message in progress, once its header is complete.
  body: Option<Zeroizing<Vec<u8>>>,
  /// Its announced length.
  expected: usize,
}

impl Reassembler {
  /// Takes the next chunk. Returns the message once its last chunk is
  /// in.
  ///
  /// # Errors
  ///
  /// [`WireError::MessageTooLarge`] as soon as a message announces more
  /// than [`MAX_MESSAGE`] bytes; [`WireError::TrailingBytes`] if a
  /// chunk goes past the end of its message.
  pub fn push(&mut self, chunk: &[u8]) -> Result<Option<Zeroizing<Vec<u8>>>, WireError> {
    let mut rest = chunk;
    let body = if let Some(body) = &mut self.body {
      body
    } else {
      let take = (MESSAGE_HEADER_LEN - self.header.len()).min(rest.len());
      self.header.extend_from_slice(&rest[..take]);
      rest = &rest[take..];
      let Ok(header) = <[u8; MESSAGE_HEADER_LEN]>::try_from(self.header.as_slice()) else {
        return Ok(None);
      };
      // A u32 always fits in usize on the 32- and 64-bit targets Arsu
      // builds for; anything that does not is too large anyway.
      let expected = usize::try_from(u32::from_be_bytes(header)).unwrap_or(usize::MAX);
      checked_length(expected)?;
      self.expected = expected;
      self
        .body
        .insert(Zeroizing::new(Vec::with_capacity(expected)))
    };
    if body.len() + rest.len() > self.expected {
      return Err(WireError::TrailingBytes);
    }
    body.extend_from_slice(rest);
    if body.len() < self.expected {
      return Ok(None);
    }
    self.header.clear();
    Ok(self.body.take())
  }
}

#[cfg(test)]
mod tests;
