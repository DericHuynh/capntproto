use capnp::{any_pointer, message, ErrorKind, Word};

fn copy(words: &[Word], limit: usize) -> capnp::Result<()> {
    let segments = [Word::words_to_bytes(words)];
    let source = message::Reader::new(
        message::SegmentArray::new(&segments),
        message::ReaderOptions {
            traversal_limit_in_words: Some(limit),
            ..Default::default()
        },
    );
    let root = source.get_root::<any_pointer::Reader<'_>>()?;
    let mut destination = message::Builder::new_default();
    destination.set_root(root)
}

#[test]
fn copying_nested_structs_and_lists_charges_every_extent_once() {
    let cases: &[(&[Word], usize)] = &[
        (
            &[
                capnp::word(1, 0, 0, 0, 66, 0, 0, 0),
                capnp::word(1, 2, 3, 4, 5, 6, 7, 8),
            ],
            2,
        ),
        (
            &[
                capnp::word(0, 0, 0, 0, 1, 0, 1, 0),
                capnp::word(7, 0, 0, 0, 0, 0, 0, 0),
                capnp::word(1, 0, 0, 0, 66, 0, 0, 0),
                capnp::word(1, 2, 3, 4, 5, 6, 7, 8),
            ],
            4,
        ),
        (
            &[
                capnp::word(1, 0, 0, 0, 14, 0, 0, 0),
                capnp::word(1, 0, 0, 0, 66, 0, 0, 0),
                capnp::word(1, 2, 3, 4, 5, 6, 7, 8),
            ],
            3,
        ),
        (
            &[
                capnp::word(1, 0, 0, 0, 23, 0, 0, 0),
                capnp::word(8, 0, 0, 0, 1, 0, 0, 0),
                capnp::word(7, 0, 0, 0, 0, 0, 0, 0),
                capnp::word(9, 0, 0, 0, 0, 0, 0, 0),
            ],
            4,
        ),
    ];
    for (words, budget) in cases {
        copy(words, *budget).unwrap();
        assert_eq!(
            copy(words, budget - 1).unwrap_err().kind,
            ErrorKind::ReadLimitExceeded
        );
    }
    copy(&[capnp::word(1, 0, 0, 0, 2, 0, 0, 0)], 1).unwrap(); // empty list at segment end
    copy(&[capnp::word(252, 255, 255, 255, 0, 0, 0, 0)], 1).unwrap(); // empty struct at negative offset
}

#[test]
fn copying_rejects_escaped_targets_and_missing_inline_tags_before_reading_them() {
    for offset in [i32::MIN >> 2, -2, 1, i32::MAX >> 2] {
        for (kind, upper) in [(0u32, 1u32), (1, 66), (1, 7)] {
            let tag = u64::from(((offset as u32) << 2) | kind) | (u64::from(upper) << 32);
            let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0)];
            Word::words_to_bytes_mut(&mut words).copy_from_slice(&tag.to_le_bytes());
            assert_eq!(
                copy(&words, 1024).unwrap_err().kind,
                ErrorKind::MessageContainsOutOfBoundsPointer
            );
        }
    }
    // A zero-word inline list still needs a tag; one-past-end is invalid.
    assert_eq!(
        copy(&[capnp::word(1, 0, 0, 0, 7, 0, 0, 0)], 1024)
            .unwrap_err()
            .kind,
        ErrorKind::MessageContainsOutOfBoundsPointer
    );
}
