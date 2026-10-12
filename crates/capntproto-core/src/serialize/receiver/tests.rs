use super::*;
use crate::{message::ReaderSegments, word, Word};

const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);

#[test]
fn every_fragment_size_preserves_frames_and_reuses_storage() {
    // Two segments, with nonzero ignored padding; then an empty second frame.
    let wire = [
        word(1, 0, 0, 0, 1, 0, 0, 0),
        word(1, 0, 0, 0, 9, 8, 7, 6),
        ZERO,
        ZERO,
        ZERO,
    ];
    let wire = Word::words_to_bytes(&wire);
    for chunk in 1..=wire.len() {
        let mut buffer = [ZERO; 5];
        let mut receiver =
            FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), Default::default()).unwrap();
        let mut source = wire;
        for expected in [32, 8] {
            while let Some(mut input) = receiver.input().unwrap() {
                let n = chunk.min(input.bytes().len()).min(source.len());
                input.bytes()[..n].copy_from_slice(&source[..n]);
                source = &source[n..];
                input.commit(n).unwrap();
            }
            assert_eq!(receiver.received_bytes(), expected);
            let message = receiver.message().unwrap().unwrap();
            assert_eq!(
                message.get_segments().len(),
                if expected == 32 { 2 } else { 1 }
            );
            assert_eq!(
                message.get_segments().get_segment(0).unwrap().len(),
                if expected == 32 { 8 } else { 0 }
            );
            receiver.next_message().unwrap();
        }
        assert!(source.is_empty());
        receiver.input().unwrap().unwrap().commit(0).unwrap();
        assert!(receiver.message().unwrap().is_none());
        assert!(receiver.into_message().unwrap().is_none());
    }
}

#[test]
fn every_truncation_and_invalid_completion_is_terminal() {
    let frame = [word(0, 0, 0, 0, 2, 0, 0, 0), ZERO, ZERO];
    let frame = Word::words_to_bytes(&frame);
    for length in 0..frame.len() {
        let mut buffer = [ZERO; 3];
        let mut receiver =
            FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), Default::default()).unwrap();
        let mut source = &frame[..length];
        let error = loop {
            let mut input = receiver.input().unwrap().unwrap();
            let n = source.len().min(input.bytes().len());
            input.bytes()[..n].copy_from_slice(&source[..n]);
            source = &source[n..];
            if let Err(error) = input.commit(n) {
                break Some(error.kind);
            }
            if n == 0 {
                break None;
            }
        };
        if length == 0 {
            assert!(error.is_none());
        } else {
            assert_eq!(error, Some(ErrorKind::PrematureEndOfFile));
            receiver.abort(ErrorKind::Failed);
            assert_eq!(
                receiver.message().err().unwrap().kind,
                ErrorKind::PrematureEndOfFile
            );
            assert_eq!(
                receiver.input().err().unwrap().kind,
                ErrorKind::PrematureEndOfFile
            );
            assert_eq!(
                receiver.next_message().unwrap_err().kind,
                ErrorKind::PrematureEndOfFile
            );
            assert_eq!(
                receiver.into_message().err().unwrap().kind,
                ErrorKind::PrematureEndOfFile
            );
        }
    }
    let mut buffer = [ZERO; 2];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), Default::default()).unwrap();
    assert_eq!(
        receiver.message().err().unwrap().kind,
        ErrorKind::IncompleteFill
    );
    assert_eq!(
        receiver.next_message().unwrap_err().kind,
        ErrorKind::IncompleteFill
    );
    {
        let _pending = receiver.input().unwrap().unwrap(); // Pending IO commits nothing.
    }
    assert_eq!(receiver.received_bytes(), 0);
    assert_eq!(
        receiver
            .input()
            .unwrap()
            .unwrap()
            .commit(9)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidReadCount(9, 8)
    );
    assert_eq!(receiver.received_bytes(), 0);
}

#[test]
fn validation_rejects_hostile_headers_limits_and_capacity_before_body_io() {
    for (header, limit, expected) in [
        (
            word(255, 255, 255, 255, 0, 0, 0, 0),
            None,
            ErrorKind::InvalidNumberOfSegments(0),
        ),
        (
            word(255, 1, 0, 0, 0, 0, 0, 0),
            None,
            ErrorKind::InvalidNumberOfSegments(512),
        ),
        (
            word(3, 0, 0, 0, 0, 0, 0, 0),
            None,
            ErrorKind::BufferNotLargeEnough,
        ),
        (
            word(0, 0, 0, 0, 2, 0, 0, 0),
            Some(1),
            ErrorKind::MessageTooLarge(2),
        ),
        (
            word(0, 0, 0, 0, 2, 0, 0, 0),
            None,
            ErrorKind::BufferNotLargeEnough,
        ),
    ] {
        let mut buffer = [ZERO; 2];
        let mut options = message::ReaderOptions::new();
        options.traversal_limit_in_words = limit;
        let mut receiver =
            FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), options).unwrap();
        let mut input = receiver.input().unwrap().unwrap();
        input
            .bytes()
            .copy_from_slice(Word::words_to_bytes(&[header]));
        assert_eq!(input.commit(8).unwrap_err().kind, expected);
        assert_eq!(receiver.received_bytes(), 8);
        assert_eq!(receiver.input().err().unwrap().kind, expected);
    }
    let mut buffer = [ZERO; 2];
    assert_eq!(
        FrameReceiver::new(&mut [], Default::default())
            .err()
            .unwrap()
            .kind,
        ErrorKind::BufferNotLargeEnough
    );
    if !cfg!(feature = "unaligned") {
        assert_eq!(
            FrameReceiver::new(
                &mut Word::words_to_bytes_mut(&mut buffer)[1..],
                Default::default()
            )
            .err()
            .unwrap()
            .kind,
            ErrorKind::UnalignedSegment
        );
    }
    assert!(SegmentTableHeader::parse(&[0; 7]).is_err());
    let header = SegmentTableHeader::parse(&[254, 1, 0, 0, 255, 255, 255, 255]).unwrap();
    assert_eq!(header.segment_count(), 511);
    assert_eq!(header.table_bytes(), 2048);
    assert_eq!(header.first_segment_words(), u32::MAX as usize);
}

#[test]
fn segment_tables_accept_even_odd_and_maximum_counts_with_empty_segments() {
    for count in [1usize, 2, 3, 4, 510, 511] {
        let mut wire = [ZERO; 512];
        let bytes = Word::words_to_bytes_mut(&mut wire);
        bytes[..4].copy_from_slice(&(count as u32 - 1).to_le_bytes());
        // Only the last segment has data; lookup must skip all empty segments.
        bytes[count * 4..count * 4 + 4].copy_from_slice(&1u32.to_le_bytes());
        let table = (count / 2 + 1) * 8;
        bytes[table..table + 8].fill(42);
        for chunk in [1usize, 7, 4096] {
            let mut buffer = [ZERO; 512];
            let mut receiver =
                FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), Default::default())
                    .unwrap();
            let mut source = &bytes[..table + 8];
            while let Some(mut window) = receiver.input().unwrap() {
                let n = chunk.min(window.bytes().len()).min(source.len());
                window.bytes()[..n].copy_from_slice(&source[..n]);
                source = &source[n..];
                window.commit(n).unwrap();
            }
            assert!(source.is_empty());
            let message = receiver.into_message().unwrap().unwrap();
            let segments = message.get_segments();
            assert_eq!(segments.len(), count);
            for id in 0..count {
                assert_eq!(
                    segments.get_segment(id as u32).unwrap(),
                    if id + 1 == count { &[42; 8][..] } else { &[] }
                );
            }
            assert_eq!(segments.get_segment(count as u32), None);
        }
    }
}
