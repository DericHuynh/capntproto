//! Index validation shared by ordinary and allocation-free payload copies.
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some(mapped) => source < count && mapped == source,
    None => source >= count,
}))]
pub(super) fn identity(source: u32, count: u32) -> Option<u32> {
    if source < count {
        Some(source)
    } else {
        None
    }
}

#[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (forall|i: int| 0 <= i < indices.len() ==> indices[i] < count)))]
pub(super) fn valid(indices: &[u32], count: u32) -> bool {
    let mut index = 0;
    #[cfg_attr(capntproto_verus, verus_spec(
        invariant index <= indices.len(), forall|i: int| 0 <= i < index ==> indices[i] < count,
        decreases indices.len() - index,
    ))]
    while index < indices.len() {
        if indices[index] >= count {
            return false;
        }
        index += 1;
    }
    true
}
