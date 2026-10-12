use capnp::{any_pointer, data, message, primitive_list, text, ErrorKind, Word};

#[test]
fn near_byte_lists_charge_their_full_extent_exactly_once() {
    let words = [
        capnp::word(1, 0, 0, 0, 66, 0, 0, 0), // eight bytes at word one
        capnp::word(b'a', b'b', b'c', b'd', b'e', b'f', b'g', 0),
    ];
    for kind in 0..3 {
        let segments = [Word::words_to_bytes(&words)];
        let reader = message::Reader::new(
            message::SegmentArray::new(&segments),
            message::ReaderOptions {
                traversal_limit_in_words: Some(2), // root plus list
                ..Default::default()
            },
        );
        let root = reader.get_root::<any_pointer::Reader>().unwrap();
        match kind {
            0 => assert_eq!(root.get_as::<data::Reader>().unwrap(), b"abcdefg\0"),
            1 => assert_eq!(
                root.get_as::<text::Reader>().unwrap().to_str().unwrap(),
                "abcdefg"
            ),
            _ => assert_eq!(
                root.get_as::<primitive_list::Reader<u8>>().unwrap().len(),
                8
            ),
        }
        assert_eq!(
            root.get_as::<data::Reader>().unwrap_err().kind,
            ErrorKind::ReadLimitExceeded
        );
    }
}

#[test]
fn near_list_offsets_cannot_escape_the_segment() {
    for low in [5u32, 0xffff_fff9, 0x7fff_fffd, 0x8000_0001] {
        let offset = low.to_le_bytes();
        let words = [
            capnp::word(offset[0], offset[1], offset[2], offset[3], 66, 0, 0, 0),
            capnp::word(0, 0, 0, 0, 0, 0, 0, 0),
        ];
        let segments = [Word::words_to_bytes(&words)];
        let reader =
            message::Reader::new(message::SegmentArray::new(&segments), Default::default());
        let root = reader.get_root::<any_pointer::Reader>().unwrap();
        assert_eq!(
            root.get_as::<data::Reader>().unwrap_err().kind,
            ErrorKind::MessageContainsOutOfBoundsPointer
        );
        assert_eq!(
            root.get_as::<text::Reader>().unwrap_err().kind,
            ErrorKind::MessageContainsOutOfBoundsPointer
        );
        assert_eq!(
            root.get_as::<primitive_list::Reader<u8>>()
                .err()
                .unwrap()
                .kind,
            ErrorKind::MessageContainsOutOfBoundsPointer
        );
    }
}

#[test]
fn empty_near_list_may_point_one_past_the_segment() {
    let words = [capnp::word(1, 0, 0, 0, 2, 0, 0, 0)];
    let segments = [Word::words_to_bytes(&words)];
    let reader = message::Reader::new(message::SegmentArray::new(&segments), Default::default());
    let root = reader.get_root::<any_pointer::Reader>().unwrap();
    assert_eq!(root.get_as::<data::Reader>().unwrap(), b"");
    assert!(root
        .get_as::<primitive_list::Reader<u8>>()
        .unwrap()
        .is_empty());
    assert_eq!(
        root.get_as::<text::Reader>().unwrap_err().kind,
        ErrorKind::MessageContainsTextThatIsNotNULTerminated
    );
}
