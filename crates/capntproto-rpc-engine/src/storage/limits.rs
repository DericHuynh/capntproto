//! Full-width growth policy shared by fixed and allocating engine storage.
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some(next) => next >= current && next as int >= current as int + additional as int && next <= maximum,
    None => current as int + additional as int > maximum,
}))]
pub(crate) fn growth_target(current: usize, additional: usize, maximum: usize) -> Option<usize> {
    let required = current.checked_add(additional)?;
    if required > maximum {
        return None;
    }
    Some(required.max(current.saturating_mul(2).min(maximum)))
}
