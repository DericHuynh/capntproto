use super::*;
const ZERO: crate::Word = crate::word(0, 0, 0, 0, 0, 0, 0, 0);

#[test]
fn cached_intervals_reject_invalid_ranges_without_spending_traversal_budget() {
    let storage = [ZERO; 4];
    let bytes = crate::Word::words_to_bytes(&storage);
    let segments = [&bytes[8..24], &bytes[24..]];
    let arena = ReaderArenaImpl::new(
        &segments[..],
        message::ReaderOptions {
            traversal_limit_in_words: Some(2),
            nesting_limit: 7,
        },
    );
    let cached = arena.cached().unwrap();
    let start = segments[0].as_ptr();
    for (pointer, size) in [
        (bytes.as_ptr(), 0),
        (start.wrapping_add(17), 0),
        (start.wrapping_add(16), 1),
        (start, usize::MAX),
    ] {
        assert_eq!(
            cached.contains_interval(0, pointer, size).unwrap_err().kind,
            ErrorKind::MessageContainsOutOfBoundsPointer
        );
    }
    assert_eq!(
        cached.contains_interval(2, start, 0).unwrap_err().kind,
        ErrorKind::InvalidSegmentId(2)
    );
    assert_eq!(cached.size_in_words(), 3);
    assert_eq!(cached.nesting_limit(), 7);
    cached
        .contains_interval(0, start.wrapping_add(16), 0)
        .unwrap();
    cached.contains_interval(0, start, 1).unwrap();
    cached
        .contains_interval(1, segments[1].as_ptr(), 1)
        .unwrap();
    // Cached and ordinary views share exactly one traversal budget.
    assert!(arena.contains_interval(0, start, 1).is_err());
    assert!(cached.contains_interval(0, start, 1).is_err());
}

#[test]
fn generated_schema_intervals_cannot_overflow_length_or_end_address() {
    static WORDS: [crate::Word; 2] = [ZERO; 2];
    let arena = GeneratedCodeArena::new(&WORDS);
    let (start, _) = arena.get_segment(0).unwrap();
    for (pointer, size) in [
        (start.wrapping_sub(1), 0),
        (start.wrapping_add(17), 0),
        (start.wrapping_add(16), 1),
        (start, usize::MAX),
        (start.wrapping_add(8), usize::MAX / BYTES_PER_WORD),
    ] {
        assert_eq!(
            arena.contains_interval(0, pointer, size).unwrap_err().kind,
            ErrorKind::MessageContainsOutOfBoundsPointer
        );
    }
    assert_eq!(
        arena.get_segment(1).unwrap_err().kind,
        ErrorKind::InvalidSegmentId(1)
    );
    assert_eq!(arena.size_in_words(), 2);
    arena.contains_interval(0, start, 2).unwrap();
    arena
        .contains_interval(0, start.wrapping_add(16), 0)
        .unwrap();
}
