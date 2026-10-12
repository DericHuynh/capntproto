//! Bounded proof of the actual reservation, zeroing and reset implementation.
use super::*;

#[kani::proof]
#[kani::unwind(33)]
fn fixed_rollback_preserves_published_prefix() {
    #[repr(C, align(8))]
    struct Buffer([u8; 32]);
    let mut buffer = Buffer([0xa5; 32]);
    let capacity: u8 = kani::any();
    kani::assume(capacity > 0 && capacity <= 4);
    let mut arena = FixedArena::new(&mut buffer.0[..usize::from(capacity) * 8]).unwrap();
    let prefix: u32 = kani::any();
    kani::assume(prefix < u32::from(capacity));
    arena.allocate(0, prefix).unwrap();
    let checkpoint = arena.allocation_checkpoint().unwrap();
    // SAFETY: all prefix bytes were reserved above and no view is borrowed.
    unsafe {
        arena.base.write_bytes(0x5a, checkpoint as usize * 8);
    }
    let amount: u32 = kani::any();
    let allocation = arena.allocate(0, amount);
    let used = arena.used();
    if allocation.is_some() {
        // SAFETY: this unpublished suffix is exclusively owned by the harness.
        unsafe {
            arena
                .base
                .add(checkpoint as usize * 8)
                .write_bytes(0x77, amount as usize * 8);
        }
    }
    // SAFETY: the suffix has no published references or owned resources.
    unsafe {
        arena.rewind_allocations(checkpoint);
    }
    kani::assert(
        arena.used() == checkpoint,
        "fixed rollback restores the reservation cursor",
    );
    assert!(arena.bytes().iter().all(|&b| b == 0x5a));
    // SAFETY: the whole borrowed buffer stays initialized and live; the slice
    // is used only for inspection and does not outlive the exclusive arena.
    let bytes = unsafe { slice::from_raw_parts(arena.base, usize::from(capacity) * 8) };
    assert!(bytes[checkpoint as usize * 8..used as usize * 8]
        .iter()
        .all(|&b| b == 0));
    assert!(bytes[used as usize * 8..].iter().all(|&b| b == 0xa5));
    assert_eq!(
        arena.allocate(0, u32::from(capacity) - checkpoint),
        Some(checkpoint)
    );
}

#[kani::proof]
#[kani::unwind(33)]
fn fixed_reservations_preserve_live_words() {
    #[repr(C, align(8))]
    struct Buffer([u8; 32]);
    let mut buffer = Buffer([0xa5; 32]);
    let capacity: u8 = kani::any();
    kani::assume(capacity > 0 && capacity <= 4);
    let mut arena = FixedArena::new(&mut buffer.0[..usize::from(capacity) * 8]).unwrap();
    // SAFETY: construction reserves the first eight bytes; no reader exists.
    // Nonzero live contents make accidental clearing observable to the proof.
    unsafe {
        arena.base.write_bytes(0x5a, 8);
    }
    for _ in 0..2 {
        let id: u32 = kani::any();
        let amount: u32 = kani::any();
        let used = arena.used();
        let remaining = arena.remaining();
        let mut before = [0; 32];
        before[..used as usize * 8].copy_from_slice(arena.bytes());
        let reservation = arena.allocate(id, amount);
        kani::assert(
            reservation.is_some() == (id == 0 && amount <= remaining),
            "fixed reservation accepts exactly available capacity",
        );
        assert!(arena.used() <= u32::from(capacity));
        assert_eq!(
            &arena.bytes()[..used as usize * 8],
            &before[..used as usize * 8]
        );
        if let Some(start) = reservation {
            assert_eq!(start, used);
            assert_eq!(arena.used(), used + amount);
            assert!(arena.bytes()[used as usize * 8..]
                .iter()
                .all(|&byte| byte == 0));
        } else {
            assert_eq!(arena.used(), used);
        }
    }
    let used = arena.used();
    arena.reset();
    assert_eq!(arena.used(), 1);
    assert_eq!(arena.bytes(), &[0; 8]);
    drop(arena);
    let bytes = &buffer.0;
    assert!(bytes[..used as usize * 8].iter().all(|&byte| byte == 0));
    assert!(bytes[used as usize * 8..].iter().all(|&byte| byte == 0xa5));
}

#[kani::proof]
#[kani::unwind(33)]
fn fixed_publication_preserves_initialized_prefix() {
    #[repr(C, align(8))]
    struct Buffer([u8; 32]);
    let mut buffer = Buffer([0xa5; 32]);
    let count: u8 = kani::any();
    kani::assume(count > 0 && count <= 4);
    let value: u8 = kani::any();
    let mut arena = FixedArena::new(&mut buffer.0).unwrap();
    arena.allocate(0, u32::from(count) - 1).unwrap();
    // SAFETY: the selected prefix was reserved and no views exist.
    unsafe {
        arena.base.write_bytes(value, usize::from(count) * 8);
    }
    let published = arena.into_bytes();
    assert_eq!(published.len(), usize::from(count) * 8);
    kani::assert(
        published.iter().all(|&byte| byte == value),
        "fixed publication preserves initialized bytes",
    );
    assert!(buffer.0[usize::from(count) * 8..]
        .iter()
        .all(|&byte| byte == 0xa5));
}
