//! Arithmetic for fixed reservations. Pointer provenance is checked separately
//! by Miri; these functions do not dereference memory or depend on an allocator.
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => requires used <= capacity, ensures match r {
    Some((start, end)) => id == 0 && start == used && end as int == used as int + amount as int && end <= capacity,
    None => id != 0 || used as int + amount as int > capacity,
}))]
pub(super) fn reserve(used: u32, capacity: u32, id: u32, amount: u32) -> Option<(u32, u32)> {
    if id != 0 || amount > capacity - used {
        None
    } else {
        Some((used, used + amount))
    }
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some(length) => checkpoint >= 1 && checkpoint <= used && length as int == used as int - checkpoint as int,
    None => checkpoint == 0 || checkpoint > used,
}))]
pub(super) fn rewind(used: u32, checkpoint: u32) -> Option<u32> {
    if checkpoint == 0 || checkpoint > used {
        None
    } else {
        Some(used - checkpoint)
    }
}
