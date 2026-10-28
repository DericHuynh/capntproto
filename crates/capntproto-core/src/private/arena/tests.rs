use super::*;
const ZERO: crate::Word = crate::word(0, 0, 0, 0, 0, 0, 0, 0);

#[test]
fn relative_offsets_larger_than_native_addresses_are_rejected() {
    static WORDS: [crate::Word; 2] = [ZERO; 2];
    let arena = GeneratedCodeArena::new(&WORDS);
    let (start, _) = arena.get_segment(0).unwrap();
    for offset in [i32::MIN, -1, 3, 0x1fff_ffff, 0x2000_0000, i32::MAX] {
        // SAFETY: start is the beginning of the arena's live segment. The
        // offset is deliberately invalid and must fail before pointer creation.
        let error = unsafe { arena.check_offset(0, start, offset) }.unwrap_err();
        assert_eq!(error.kind, ErrorKind::MessageContainsOutOfBoundsPointer);
    }
    for offset in [0, 1, 2] {
        // SAFETY: start belongs to this segment; its end is allowed but unused.
        let target = unsafe { arena.check_offset(0, start, offset) }.unwrap();
        assert_eq!(target, start.wrapping_add(offset as usize * BYTES_PER_WORD));
    }
}

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

#[cfg(feature = "alloc")]
#[test]
fn external_segments_reject_mutation_and_survive_allocator_recovery() {
    use alloc::sync::Arc;
    let owner: Arc<[crate::Word]> = Arc::from([ZERO; 2]);
    let external = crate::dynamic_orphan::ExternalData::new(owner.clone(), 16).unwrap();
    let mut arena = BuilderArenaImpl::new(message::HeapAllocator::new().first_segment_words(4));
    assert_eq!(arena.segment_for_output(0), None);
    arena.allocate_segment(4).unwrap();
    assert_eq!(arena.allocate(0, 2), Some(0));
    assert!(arena.try_resize_allocation(0, 0, 2, 3));
    assert_eq!(arena.retained_segment_bytes(), Some(32));
    let id = arena.add_external_segment(external).unwrap();
    assert_eq!(id, 1);
    assert!(!arena.is_writable(id));
    assert!(!arena.try_resize_allocation(id, 0, 2, 1));
    assert_eq!(arena.allocate(id, 0), None);
    assert_eq!(arena.allocate_ptr(id, 0), None);
    assert_eq!(
        arena.segment_for_output(id).unwrap(),
        crate::Word::words_to_bytes(&owner)
    );
    assert_eq!(arena.segment_for_output(2), None);
    assert_eq!(arena.segment_for_output(u32::MAX), None);
    assert_eq!(arena.retained_segment_bytes(), None);
    assert_eq!(Arc::strong_count(&owner), 2);
    // Recovering the allocator deallocates its own segments exactly once and
    // drops external owners through their original allocation mechanism.
    let allocator = arena.into_allocator();
    assert_eq!(Arc::strong_count(&owner), 1);
    assert_eq!(crate::Word::words_to_bytes(&owner), &[0; 16]);
    let mut reused = BuilderArenaImpl::new(allocator);
    reused.allocate_segment(1).unwrap();
    assert_eq!(reused.allocate(0, 1), Some(0));
}

#[test]
fn segment_reservations_match_wide_cursor_model() {
    for capacity in [0, 1, 7, 512, u32::MAX - 1, u32::MAX] {
        for used in [0, capacity / 2, capacity] {
            for amount in [0, 1, capacity, capacity - used, u32::MAX] {
                // The reservation touches only metadata; this address is never
                // dereferenced, including for capacities larger than real RAM.
                let mut segment = BuilderSegment {
                    #[cfg(feature = "alloc")]
                    external: None,
                    ptr: core::ptr::NonNull::<crate::Word>::dangling()
                        .as_ptr()
                        .cast(),
                    capacity,
                    allocated: used,
                };
                let end = u64::from(used) + u64::from(amount);
                let expected = (end <= u64::from(capacity)).then_some(used);
                assert_eq!(segment.allocate(amount), expected);
                assert_eq!(
                    u64::from(segment.allocated),
                    if expected.is_some() {
                        end
                    } else {
                        u64::from(used)
                    }
                );
                assert!(segment.allocated <= capacity);
                let cursor = segment.allocated;
                assert_eq!(segment.allocate(0), Some(cursor));
                assert_eq!(segment.allocated, cursor);
            }
        }
    }
}

#[cfg(feature = "alloc")]
#[test]
fn heap_reservations_return_non_null_empty_and_one_past_end_addresses() {
    for capacity in [0, 4] {
        let mut arena =
            BuilderArenaImpl::new(message::HeapAllocator::new().first_segment_words(capacity));
        arena.allocate_segment(capacity).unwrap();
        let (base, words) = arena.get_segment_mut(0);
        assert_eq!(words, capacity);
        assert!(!base.is_null());
        assert_eq!(arena.allocate_ptr(0, 0).unwrap().as_ptr(), base);
        assert_eq!(arena.allocate_ptr(0, capacity).unwrap().as_ptr(), base);
        let end = base.wrapping_add(capacity as usize * BYTES_PER_WORD);
        assert_eq!(arena.allocate_ptr(0, 0).unwrap().as_ptr(), end);
        assert_eq!(arena.allocate_ptr(0, 1), None);
        assert_eq!(arena.allocate_ptr(0, u32::MAX), None);
        assert_eq!(arena.allocate_ptr(0, 0).unwrap().as_ptr(), end);
        assert_eq!(
            arena.segment_for_output(0).unwrap().len(),
            capacity as usize * BYTES_PER_WORD
        );
    }
}

#[test]
fn fixed_reservations_preserve_empty_and_one_past_end_addresses() {
    let mut words = [ZERO; 4];
    crate::Word::words_to_bytes_mut(&mut words).fill(0xa5);
    let mut arena = fixed::FixedArena::new(crate::Word::words_to_bytes_mut(&mut words)).unwrap();
    let (base, capacity) = arena.get_segment_mut(0);
    assert_eq!(capacity, 4);
    let after_root = base.wrapping_add(BYTES_PER_WORD);
    assert_eq!(arena.allocate_ptr(0, 0).unwrap().as_ptr(), after_root);
    assert_eq!(arena.allocate_ptr(0, 3).unwrap().as_ptr(), after_root);
    let end = base.wrapping_add(4 * BYTES_PER_WORD);
    assert_eq!(arena.allocate_ptr(0, 0).unwrap().as_ptr(), end);
    assert_eq!(arena.allocate_ptr(0, 1), None);
    assert_eq!(arena.allocate_ptr(0, u32::MAX), None);
    assert_eq!(arena.allocate_ptr(0, 0).unwrap().as_ptr(), end);
    assert!(arena.bytes().iter().all(|byte| *byte == 0));
}
