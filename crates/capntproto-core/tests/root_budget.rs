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
