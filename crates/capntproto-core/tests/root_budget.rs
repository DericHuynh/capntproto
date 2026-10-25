use capnp::{
    any_pointer,
    message::{Reader, ReaderOptions},
    ErrorKind, Word,
};

#[test]
fn root_word_bounds_and_budget_are_checked_before_exposing_a_reader() {
    let words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0)];
    let bytes = Word::words_to_bytes(&words);
    for length in 0..8 {
        let segments = [&bytes[..length]];
        let reader = Reader::new(&segments[..], ReaderOptions::new());
        assert_eq!(
            reader.get_root::<any_pointer::Reader>().err().unwrap().kind,
            ErrorKind::MessageContainsOutOfBoundsPointer
        );
    }
    let segments = [bytes];
    for limit in [0, 1, 2] {
        let reader = Reader::new(
            &segments[..],
            ReaderOptions {
                traversal_limit_in_words: Some(limit),
                ..ReaderOptions::new()
            },
        );
        for _ in 0..limit {
            assert!(reader.get_root::<any_pointer::Reader>().unwrap().is_null());
        }
        assert_eq!(
            reader.get_root::<any_pointer::Reader>().err().unwrap().kind,
            ErrorKind::ReadLimitExceeded
        );
    }
}

#[test]
fn declined_and_failed_selection_recover_the_consumed_reader() {
    let words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0)];
    let segments = [Word::words_to_bytes(&words)];
    for fail in [false, true] {
        let reader = Reader::new(
            &segments[..],
            ReaderOptions {
                traversal_limit_in_words: Some(2),
                ..Default::default()
            },
        );
        let mut recovered = None;
        let selected = reader.try_select_pointer_or_else::<(), capnp::Error>(
            |_| {
                if fail {
                    Err(capnp::Error::from_kind(ErrorKind::Failed))
                } else {
                    Ok(None)
                }
            },
            |reader| recovered = Some(reader),
        );
        assert_eq!(selected.is_err(), fail);
        let reader = recovered.expect("declined or failed selection retains its owner");
        assert_eq!(reader.read_budget().remaining_words(), Some(1));
        reader.get_root::<any_pointer::Reader>().unwrap();
        assert_eq!(
            reader.get_root::<any_pointer::Reader>().err().unwrap().kind,
            ErrorKind::ReadLimitExceeded
        );
    }
    let reader = Reader::new(
        &segments[..],
        ReaderOptions {
            traversal_limit_in_words: Some(1),
            ..Default::default()
        },
    );
    let selected = reader
        .try_select_pointer_or_else::<(), capnp::Error>(
            |root| Ok(Some((root, ()))),
            |_| panic!("selected reader must not be recovered"),
        )
        .unwrap()
        .unwrap()
        .0;
    assert!(selected.root().is_null());
    let reader = Reader::new(
        &segments[..],
        ReaderOptions {
            traversal_limit_in_words: Some(0),
            ..Default::default()
        },
    );
    let mut recovered = false;
    let result = reader.try_select_pointer_or_else::<(), capnp::Error>(
        |_| panic!("exhausted root cannot call the selector"),
        |reader| {
            assert_eq!(reader.read_budget().remaining_words(), Some(0));
            recovered = true;
        },
    );
    assert_eq!(result.err().unwrap().kind, ErrorKind::ReadLimitExceeded);
    assert!(recovered);
}
