use proptest::prelude::*;
use uuid::Uuid;
use vault_core::{DeviceSet, Member};

use super::*;
use crate::{Replica, devices, noise::new_identity, session::Mode};

#[test]
fn the_uuids_never_change() {
  assert_eq!(
    SERVICE_UUID.to_string(),
    "8a8f11c2-b37f-496e-97a9-b1ba6eb6ee78"
  );
  assert_eq!(RX_UUID.to_string(), "d38b2aeb-d8f3-4c2d-bcda-8300b7fa5d42");
  assert_eq!(TX_UUID.to_string(), "45906ec1-7669-42a7-a423-266fd2995885");
}

#[test]
fn the_token_comes_from_arsus_service_data() {
  let data = std::collections::HashMap::from([(SERVICE_UUID, vec![7; 8])]);

  assert_eq!(token_of(&data), Some([7; 8]));
}

#[test]
fn no_token_without_service_data_of_the_right_length() {
  use std::collections::HashMap;

  assert_eq!(token_of(&HashMap::new()), None);
  assert_eq!(token_of(&HashMap::from([(SERVICE_UUID, vec![7; 3])])), None);
  assert_eq!(
    token_of(&HashMap::from([(Uuid::from_u128(1), vec![7; 8])])),
    None
  );
}

#[test]
fn arsu_is_recognised_by_its_service_or_by_its_service_data() {
  use std::collections::HashMap;
  let other = Uuid::from_u128(1);
  let no_data: HashMap<Uuid, Vec<u8>> = HashMap::new();

  // Waiting to sync: the service list.
  assert!(advertises_arsu(&[SERVICE_UUID], &no_data));
  assert!(advertises_arsu(&[other, SERVICE_UUID], &no_data));
  // Waiting to pair: the token, in the service's data.
  assert!(advertises_arsu(
    &[],
    &HashMap::from([(SERVICE_UUID, vec![7; 8])])
  ));
  // Anything else.
  assert!(!advertises_arsu(
    &[other],
    &HashMap::from([(other, vec![7; 8])])
  ));
  assert!(!advertises_arsu(&[], &no_data));
}

#[test]
fn a_chunk_fills_an_att_packet() {
  // MTU minus the 3-byte ATT header, minus the sequence number.
  assert_eq!(Chunker::new(23).payload_len(), 19);
  assert_eq!(Chunker::new(185).payload_len(), 181);
}

#[test]
fn a_chunk_never_exceeds_the_longest_attribute_value() {
  // Android 14 asks for 517 bytes; an attribute is at most 512.
  assert_eq!(Chunker::new(517).payload_len(), 511);
}

#[test]
fn an_mtu_below_the_minimum_counts_as_the_minimum() {
  assert_eq!(Chunker::new(0).payload_len(), 19);
}

#[test]
fn chunks_are_numbered_from_zero_and_wrap() {
  let mut chunker = Chunker::new(23);
  let chunks = chunker.chunks(&vec![7u8; 19 * 300]);

  assert_eq!(chunks.len(), 300);
  assert_eq!(chunks[0][0], 0);
  assert_eq!(chunks[255][0], 255);
  assert_eq!(chunks[256][0], 0, "wraps after 255");
  assert!(chunks.iter().all(|chunk| chunk.len() == 20));
}

#[test]
fn numbering_carries_on_across_calls() {
  let mut chunker = Chunker::new(23);
  chunker.chunks(b"first");

  assert_eq!(chunker.chunks(b"second")[0][0], 1);
}

#[test]
fn nothing_to_send_makes_no_chunk() {
  assert!(Chunker::new(23).chunks(b"").is_empty());
}

#[test]
fn chunks_out_of_order_are_refused() {
  let mut chunker = Chunker::new(23);
  let chunks = chunker.chunks(&[1u8; 60]);
  let mut dechunker = Dechunker::default();

  assert_eq!(dechunker.push(&chunks[0]).unwrap(), Some(&[1u8; 19][..]));
  assert_eq!(dechunker.push(&chunks[2]), Err(GattError::OutOfOrder));
}

#[test]
fn the_chunk_just_taken_arriving_again_is_dropped() {
  let mut chunker = Chunker::new(23);
  let chunks = chunker.chunks(&[1u8; 60]);
  let mut dechunker = Dechunker::default();
  dechunker.push(&chunks[0]).unwrap();

  assert_eq!(dechunker.push(&chunks[0]), Ok(None), "a write sent twice");
  assert_eq!(dechunker.push(&chunks[1]).unwrap(), Some(&[1u8; 19][..]));
}

#[test]
fn an_older_chunk_arriving_again_is_out_of_order() {
  let mut chunker = Chunker::new(23);
  let chunks = chunker.chunks(&[1u8; 60]);
  let mut dechunker = Dechunker::default();
  dechunker.push(&chunks[0]).unwrap();
  dechunker.push(&chunks[1]).unwrap();

  assert_eq!(dechunker.push(&chunks[0]), Err(GattError::OutOfOrder));
}

#[test]
fn a_chunk_with_the_last_number_but_other_bytes_is_out_of_order() {
  let mut dechunker = Dechunker::default();
  dechunker.push(&[0, 1, 2]).unwrap();

  assert_eq!(dechunker.push(&[0, 9, 9]), Err(GattError::OutOfOrder));
}

#[test]
fn an_empty_chunk_is_refused() {
  assert_eq!(Dechunker::default().push(&[]), Err(GattError::EmptyChunk));
}

proptest! {
  #[test]
  fn the_stream_survives_chunking(
    writes in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..2000), 1..5),
    mtu in 0..600usize,
  ) {
    let mut chunker = Chunker::new(mtu);
    let mut dechunker = Dechunker::default();
    let mut received = Vec::new();
    for write in &writes {
      for chunk in chunker.chunks(write) {
        prop_assert!(chunk.len() <= MAX_ATTRIBUTE_VALUE);
        received.extend_from_slice(dechunker.push(&chunk).unwrap().unwrap());
      }
    }
    prop_assert_eq!(received, writes.concat());
  }
}

// ---- the link ---------------------------------------------------------------

fn snapshot_of(identity: &DeviceIdentity, trusted: &[PublicKey]) -> Snapshot {
  let mut set = DeviceSet::default();
  for key in trusted {
    devices::add(
      &mut set,
      Member {
        public_key: *key,
        name: "Device".to_string(),
        added_at: 1,
      },
    )
    .unwrap();
  }
  Snapshot::new(
    identity.name.clone(),
    Replica {
      vault_id: Uuid::from_u128(1),
      entries: Vec::new(),
      tombstones: Vec::new(),
    },
    set,
  )
}

/// One side of a link: chunks to deliver, what it received, whether it
/// finished.
struct End {
  link: GattLink,
  outbox: Vec<Vec<u8>>,
  received: Vec<PublicKey>,
  finished: bool,
}

impl End {
  fn new(link: GattLink, outputs: Vec<LinkOutput>) -> Self {
    let mut end = Self {
      link,
      outbox: Vec::new(),
      received: Vec::new(),
      finished: false,
    };
    end.take(outputs);
    end
  }

  fn take(&mut self, outputs: Vec<LinkOutput>) {
    for output in outputs {
      match output {
        LinkOutput::Chunk(chunk) => self.outbox.push(chunk),
        LinkOutput::Received { peer, .. } => {
          self.received.push(peer);
          let more = self.link.acknowledge().unwrap();
          self.take(more);
        }
        LinkOutput::Finished => self.finished = true,
      }
    }
  }

  fn deliver(&mut self, chunks: Vec<Vec<u8>>) {
    for chunk in chunks {
      let outputs = self.link.receive(&chunk).unwrap();
      self.take(outputs);
    }
  }
}

#[test]
fn a_sync_runs_over_gatt_chunks_of_different_sizes() {
  let (computer, phone) = (
    new_identity(Uuid::now_v7(), "Computer".to_string()).unwrap(),
    new_identity(Uuid::now_v7(), "Phone".to_string()).unwrap(),
  );
  let both = [computer.public_key, phone.public_key];
  let (link, outputs) = GattLink::new(
    Mode::Sync { initiator: true },
    &phone,
    snapshot_of(&phone, &both),
    517,
  )
  .unwrap();
  let mut central = End::new(link, outputs);
  let (link, outputs) = GattLink::new(
    Mode::Sync { initiator: false },
    &computer,
    snapshot_of(&computer, &both),
    23,
  )
  .unwrap();
  let mut peripheral = End::new(link, outputs);

  assert!(
    central.outbox.iter().all(|chunk| chunk.len() <= 512),
    "the central's chunks fit its MTU"
  );
  while !(central.outbox.is_empty() && peripheral.outbox.is_empty()) {
    let to_peripheral = std::mem::take(&mut central.outbox);
    peripheral.deliver(to_peripheral);
    let to_central = std::mem::take(&mut peripheral.outbox);
    central.deliver(to_central);
  }

  assert!(central.finished && peripheral.finished);
  assert_eq!(central.received, [computer.public_key]);
  assert_eq!(peripheral.received, [phone.public_key]);
  assert_eq!(central.link.peer(), Some(computer.public_key));
}

#[test]
fn a_reordered_chunk_ends_the_link() {
  let (a, b) = (
    new_identity(Uuid::now_v7(), "A".to_string()).unwrap(),
    new_identity(Uuid::now_v7(), "B".to_string()).unwrap(),
  );
  let (_, outputs) = GattLink::new(
    Mode::Sync { initiator: true },
    &a,
    snapshot_of(&a, &[a.public_key, b.public_key]),
    23,
  )
  .unwrap();
  let (mut responder, _) = GattLink::new(
    Mode::Sync { initiator: false },
    &b,
    snapshot_of(&b, &[a.public_key, b.public_key]),
    23,
  )
  .unwrap();
  let LinkOutput::Chunk(first) = &outputs[0] else {
    panic!("the initiator starts with a chunk");
  };
  let mut skipped = first.clone();
  skipped[0] = 1;

  assert!(matches!(
    responder.receive(&skipped),
    Err(LinkError::Gatt(GattError::OutOfOrder))
  ));
}
