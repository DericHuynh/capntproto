//! Independent wire fixtures: fragmentation must not change framing or segments.
use capnp::message::{ReaderOptions, ReaderSegments};
use std::io::{BufReader, Cursor, Read};

struct Fragmented {
    input: Cursor<Vec<u8>>,
    chunk: usize,
}
impl Read for Fragmented {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = bytes.len().min(self.chunk);
        self.input.read(&mut bytes[..count])
    }
}
#[repr(align(8))]
struct Buffer([u8; 512]);

fn fixture(count: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let segments: Vec<Vec<u8>> = (0..count)
        .map(|i| (0..(i + 1) * 8).map(|j| (i * 31 + j) as u8).collect())
        .collect();
    let mut wire = ((count - 1) as u32).to_le_bytes().to_vec();
    for segment in &segments {
        wire.extend_from_slice(&((segment.len() / 8) as u32).to_le_bytes());
    }
    if count.is_multiple_of(2) {
        wire.extend_from_slice(&[0; 4]);
    }
    for segment in &segments {
        wire.extend_from_slice(segment);
    }
    (wire, segments)
}

#[test]
fn fragmented_no_alloc_reads_preserve_segments_and_next_message() {
    for count in 1..=5 {
        let (wire, expected) = fixture(count);
        for chunk in 1..=16 {
            let mut input = Fragmented {
                input: Cursor::new([wire.as_slice(), wire.as_slice()].concat()),
                chunk,
            };
            // Old table/segment bytes must not influence the next read.
            let mut buffer = Buffer([0xa5; 512]);
            for message in 1..=2 {
                let reader = capnp::serialize::try_read_message_no_alloc(
                    &mut input,
                    &mut buffer.0,
                    ReaderOptions::new(),
                )
                .unwrap()
                .unwrap();
                let segments = reader.into_segments();
                for (i, expected) in expected.iter().enumerate() {
                    assert_eq!(
                        segments.get_segment(i as u32).unwrap(),
                        expected,
                        "segments={count}, chunk={chunk}, message={message}"
                    );
                }
                assert_eq!(input.input.position(), (message * wire.len()) as u64);
            }
            assert!(capnp::serialize::try_read_message_no_alloc(
                &mut input,
                &mut buffer.0,
                ReaderOptions::new(),
            )
            .unwrap()
            .is_none());
        }
    }
}

#[test]
fn no_alloc_rejects_every_truncated_table_and_body() {
    for count in 1..=5 {
        let (wire, _) = fixture(count);
        for end in 1..wire.len() {
            let mut input = Fragmented {
                input: Cursor::new(wire[..end].to_vec()),
                chunk: 3,
            };
            let mut buffer = Buffer([0; 512]);
            assert!(
                capnp::serialize::try_read_message_no_alloc(
                    &mut input,
                    &mut buffer.0,
                    ReaderOptions::new(),
                )
                .is_err(),
                "segments={count}, truncated at {end}"
            );
        }
    }
}

#[test]
fn packed_zero_run_spanning_segment_lengths_preserves_next_message() {
    // Independent minimal fixture: four empty segments. Tag 1 emits the count
    // minus one (3); tag 0 plus run count 1 covers the next TWO table words.
    // The old no-alloc reader split that run and rejected a valid message.
    let packet = [1, 3, 0, 1];
    for chunk in 1..=5 {
        let mut input = BufReader::with_capacity(chunk, Cursor::new(packet.repeat(2)));
        let mut buffer = Buffer([0xa5; 512]);
        for _ in 0..2 {
            let reader = capnp::serialize_packed::try_read_message_no_alloc(
                &mut input,
                &mut buffer.0,
                ReaderOptions::new(),
            )
            .unwrap()
            .unwrap();
            let segments = reader.into_segments();
            for id in 0..4 {
                assert_eq!(segments.get_segment(id), Some(&[][..]));
            }
            assert!(segments.get_segment(4).is_none());
        }
        assert!(capnp::serialize_packed::try_read_message_no_alloc(
            &mut input,
            &mut buffer.0,
            ReaderOptions::new(),
        )
        .unwrap()
        .is_none());
    }
    for end in 1..packet.len() {
        let mut buffer = Buffer([0xa5; 512]);
        assert!(capnp::serialize_packed::try_read_message_no_alloc(
            &packet[..end],
            &mut buffer.0,
            ReaderOptions::new(),
        )
        .is_err());
    }
}
