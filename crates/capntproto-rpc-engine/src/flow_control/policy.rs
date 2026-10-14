//! Pure admission policies; cell ownership, wakeups and clocks stay in adapters.
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some(next) => used < limit && next as int == used as int + 1,
    None => used >= limit,
}))]
pub(crate) fn acquire(used: usize, limit: usize) -> Option<usize> {
    if used >= limit {
        None
    } else {
        Some(used + 1)
    }
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => requires amount <= used, ensures r as int == used as int - amount as int))]
pub(crate) fn release(used: usize, amount: usize) -> usize {
    used - amount
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (if was_paused { used >= limit } else { used > limit })))]
pub(crate) fn paused(used: usize, limit: usize, was_paused: bool) -> bool {
    if was_paused {
        used >= limit
    } else {
        used > limit
    }
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some((total, next_paused)) => total as int == used as int + amount as int && next_paused == (was_paused || total > limit),
    None => used as int + amount as int > usize::MAX,
}))]
pub(crate) fn charge(
    used: usize,
    amount: usize,
    limit: usize,
    was_paused: bool,
) -> Option<(usize, bool)> {
    let total = used.checked_add(amount)?;
    Some((total, was_paused || total > limit))
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures r ==
    ((!adaptive && used <= largest) || used < largest || (used as int) - (largest as int) < (size as int))
))]
pub(crate) fn ready(adaptive: bool, used: u128, largest: u128, size: usize) -> bool {
    (!adaptive && used <= largest) || used < largest || used - largest < size as u128
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some((total, maximum)) => total as int == used as int + amount as int
        && maximum == (if largest >= amount { largest } else { amount }),
    None => used as int + amount as int > u128::MAX,
}))]
pub(crate) fn sent(used: u128, largest: u128, amount: u128) -> Option<(u128, u128)> {
    Some((used.checked_add(amount)?, largest.max(amount)))
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match r {
    Some(left) => amount <= used && left as int == used as int - amount as int,
    None => amount > used,
}))]
pub(crate) fn acknowledged(used: u128, amount: u128) -> Option<u128> {
    used.checked_sub(amount)
}
