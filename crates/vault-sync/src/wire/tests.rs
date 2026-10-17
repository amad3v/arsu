use proptest::prelude::*;

use super::*;

// ---- frames -----------------------------------------------------------------

#[test]
fn a_frame_is_the_message_after_its_big_endian_length() {
  assert_eq!(frame(b"abc").unwrap(), [0, 3, b'a', b'b', b'c']);
}

#[test]
fn empty_and_oversized_messages_cannot_be_framed() {
  assert_eq!(frame(b""), Err(WireError::BadFrameLength));
  assert_eq!(
    frame(&vec![0; MAX_NOISE_MESSAGE + 1]),
    Err(WireError::BadFrameLength)
  );
  assert_eq!(
    frame(&vec![0; MAX_NOISE_MESSAGE]).unwrap().len(),
    2 + MAX_NOISE_MESSAGE
  );
}

#[test]
fn the_reader_returns_every_message_a_push_completes() {
  let mut stream = frame(b"one").unwrap();
  stream.extend(frame(b"two").unwrap());

  let mut reader = FrameReader::default();

  assert_eq!(
    reader.push(&stream).unwrap(),
    [b"one".to_vec(), b"two".to_vec()]
  );
}

#[test]
fn the_reader_waits_for_the_rest_of_a_message() {
  let stream = frame(b"hello").unwrap();
  let mut reader = FrameReader::default();

  assert!(
    reader.push(&stream[..1]).unwrap().is_empty(),
    "half a header"
  );
  assert!(
    reader.push(&stream[1..4]).unwrap().is_empty(),
    "half a body"
  );
  assert_eq!(reader.push(&stream[4..]).unwrap(), [b"hello".to_vec()]);
}

#[test]
fn the_reader_tells_whether_part_of_a_message_is_waiting() {
  let stream = frame(b"hello").unwrap();
  let mut reader = FrameReader::default();
  assert!(reader.is_empty());

  reader.push(&stream[..3]).unwrap();
  assert!(!reader.is_empty());

  reader.push(&stream[3..]).unwrap();
  assert!(reader.is_empty());
}

#[test]
fn a_frame_announcing_an_empty_message_is_an_error() {
  let mut reader = FrameReader::default();

  assert_eq!(reader.push(&[0, 0]), Err(WireError::BadFrameLength));
}

proptest! {
  /// However the link cuts the stream, the same messages come out.
  #[test]
  fn frames_survive_any_split_of_the_stream(
    messages in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 1..300), 1..6),
    cuts in proptest::collection::vec(1..64usize, 1..40),
  ) {
    let stream: Vec<u8> = messages.iter().flat_map(|m| frame(m).unwrap()).collect();
    let mut reader = FrameReader::default();
    let mut received = Vec::new();
    let mut rest = stream.as_slice();
    for cut in cuts.iter().cycle() {
      if rest.is_empty() {
        break;
      }
      let (now, later) = rest.split_at((*cut).min(rest.len()));
      received.extend(reader.push(now).unwrap());
      rest = later;
    }
    prop_assert_eq!(received, messages);
  }
}

// ---- chunks -----------------------------------------------------------------

fn reassemble(chunks: &[Zeroizing<Vec<u8>>]) -> Vec<Vec<u8>> {
  let mut reassembler = Reassembler::default();
  chunks
    .iter()
    .filter_map(|chunk| reassembler.push(chunk).unwrap())
    .map(|message| message.to_vec())
    .collect()
}

#[test]
fn a_small_message_is_one_chunk_after_its_length() {
  let chunks = split(b"hi").unwrap();

  assert_eq!(chunks.len(), 1);
  assert_eq!(chunks[0].as_slice(), [0, 0, 0, 2, b'h', b'i']);
}

#[test]
fn an_empty_message_is_its_length_alone() {
  let chunks = split(b"").unwrap();

  assert_eq!(chunks.len(), 1);
  assert_eq!(reassemble(&chunks), [Vec::<u8>::new()]);
}

#[test]
fn every_chunk_fits_in_one_noise_message() {
  let message = vec![7u8; 3 * MAX_CHUNK];
  let chunks = split(&message).unwrap();

  assert_eq!(
    chunks.len(),
    4,
    "3 chunks of message, plus the 4-byte header"
  );
  assert!(chunks.iter().all(|chunk| chunk.len() <= MAX_CHUNK));
  assert_eq!(reassemble(&chunks), [message]);
}

#[test]
fn a_message_filling_exactly_one_chunk_is_one_chunk() {
  let message = vec![1u8; MAX_CHUNK - 4];

  assert_eq!(split(&message).unwrap().len(), 1);
}

#[test]
fn messages_come_out_one_after_the_other() {
  let mut chunks = split(b"first").unwrap();
  chunks.extend(split(b"second").unwrap());

  assert_eq!(reassemble(&chunks), [b"first".to_vec(), b"second".to_vec()]);
}

#[test]
fn a_message_above_the_limit_is_refused_both_ways() {
  assert_eq!(
    split(&vec![0; MAX_MESSAGE + 1]).map(|chunks| chunks.len()),
    Err(WireError::MessageTooLarge(MAX_MESSAGE + 1))
  );

  let announced = u32::try_from(MAX_MESSAGE + 1).unwrap().to_be_bytes();
  let mut reassembler = Reassembler::default();
  assert_eq!(
    reassembler.push(&announced).map(|done| done.is_some()),
    Err(WireError::MessageTooLarge(MAX_MESSAGE + 1)),
    "refused on the header, before anything is buffered"
  );
}

#[test]
fn a_chunk_running_past_its_message_is_an_error() {
  let mut reassembler = Reassembler::default();

  assert_eq!(
    reassembler
      .push(&[0, 0, 0, 1, b'a', b'b'])
      .map(|done| done.is_some()),
    Err(WireError::TrailingBytes)
  );
}

#[test]
fn a_header_split_across_chunks_is_understood() {
  let mut reassembler = Reassembler::default();

  assert!(reassembler.push(&[0, 0]).unwrap().is_none());
  assert_eq!(
    reassembler.push(&[0, 1, b'x']).unwrap().map(|m| m.to_vec()),
    Some(b"x".to_vec())
  );
}
