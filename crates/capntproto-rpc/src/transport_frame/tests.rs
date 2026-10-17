use super::*;
use capntproto_rpc_engine::{connection::Growth, messages};
use std::{ops::Range, rc::Rc};

struct Block {
    words: Vec<capnp::Word>,
    range: Range<usize>,
}
impl Block {
    fn new(bytes: &[u8], offset: usize) -> Self {
        let mut words = capnp::Word::allocate_zeroed_vec((bytes.len() + offset).div_ceil(8));
        let range = offset..offset + bytes.len();
        capnp::Word::words_to_bytes_mut(&mut words)[range.clone()].copy_from_slice(bytes);
        Self { words, range }
    }
}
struct Source(Vec<Block>);
impl messages::Parts for Source {
    fn count(&self) -> usize {
        self.0.len()
    }
    fn part(&self, index: usize) -> &[u8] {
        let block = &self.0[index];
        &capnp::Word::words_to_bytes(&block.words)[block.range.clone()]
    }
    fn retained_capacity(&self) -> usize {
        self.0.iter().map(|b| b.words.capacity() * 8).sum()
    }
}
fn retain(source: Rc<Source>, consumed: usize) -> messages::SharedParts {
    let mut pool = messages::GrowingBuffers::new(2, 512, 1024);
    messages::with_growing_messages(
        &mut pool,
        Growth {
            initial: 0,
            maximum: 0,
        },
        |messages| {
            let parts = messages.retain_parts(source).unwrap().unwrap();
            if consumed == 0 {
                return parts;
            }
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            queue.reserve().unwrap().accept_parts(parts);
            assert!(!queue
                .window(messages)
                .unwrap()
                .unwrap()
                .advance(consumed)
                .unwrap());
            queue
                .window(messages)
                .unwrap()
                .unwrap()
                .share_parts()
                .unwrap()
        },
    )
    .unwrap()
}

#[test]
fn retained_frames_reject_bad_tables_extents_alignment_and_consumed_prefixes() {
    let table = [0, 0, 0, 0, 1, 0, 0, 0];
    let data = [0xa5; 8];
    let cases = [
        (vec![], 0),
        (vec![Block::new(&table, 0)], 0),
        (vec![Block::new(&table, 0), Block::new(&data, 0)], 1),
        (vec![Block::new(&table[..7], 0), Block::new(&data, 0)], 0),
        (vec![Block::new(&[0; 16], 0), Block::new(&data, 0)], 0),
        (
            vec![
                Block::new(&table, 0),
                Block::new(&data, 0),
                Block::new(&[], 0),
            ],
            0,
        ),
        (vec![Block::new(&table, 0), Block::new(&data[..7], 0)], 0),
        (vec![Block::new(&table, 0), Block::new(&data, 1)], 0),
    ];
    for (blocks, consumed) in cases {
        let source = Rc::new(Source(blocks));
        let parts = retain(source.clone(), consumed);
        assert_eq!(Rc::strong_count(&source), 2);
        assert!(OutgoingFrame::from_parts(parts).is_err());
        assert_eq!(
            Rc::strong_count(&source),
            1,
            "rejection releases the retained owner"
        );
    }
}

#[test]
fn retained_frames_keep_exact_aligned_segments_after_the_pool_is_gone() {
    for segments in [
        vec![vec![0xa5; 8]],
        vec![vec![0xa5; 8], vec![], vec![0x5a; 16]],
    ] {
        let encoded = capnp::serialize::write_message_segments_to_words(&segments);
        let header = capnp::serialize::SegmentTableHeader::parse(&encoded).unwrap();
        let mut blocks = vec![Block::new(&encoded[..header.table_bytes()], 0)];
        blocks.extend(segments.iter().map(|bytes| Block::new(bytes, 0)));
        let source = Rc::new(Source(blocks));
        let retained = retain(source.clone(), 0);
        let frame = OutgoingFrame::from_parts(retained).unwrap();
        let reader = frame.body.segments();
        assert_eq!(reader.len(), segments.len());
        assert_eq!(reader.get_segment(segments.len() as u32), None);
        assert_eq!(reader.get_segment(u32::MAX), None);
        assert_eq!(
            frame.size_in_words(),
            segments.iter().map(|s| s.len() / 8).sum()
        );
        assert!(frame.framed_bytes().is_none());
        assert_eq!(
            capnp::serialize::write_message_segments_to_words(&frame.segments()),
            encoded
        );
        assert_eq!(Rc::strong_count(&source), 2);
        drop(frame);
        assert_eq!(Rc::strong_count(&source), 1);
    }
}
