//! Exhaustible, non-recycling route sequence. The caller owns storage and polling.
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("verified/sequence.rs");

/// Consume one nonzero identifier; exhaustion is sticky, including after MAX.
#[cfg_attr(capntproto_verus, verus_spec(r =>
    requires valid(*old(next)),
    ensures allocation(*old(next), *final(next), r)))]
pub(crate) fn allocate(next: &mut Option<u64>) -> Option<u64> {
    let value = (*next)?;
    *next = value.checked_add(1);
    Some(value)
}
