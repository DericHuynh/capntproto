//! Buffer reference and byte-progress decisions without allocator or IO effects.
use super::Error;
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Free,
    Writing,
    Sealed {
        length: usize,
        readers: u32,
        sends: u32,
    },
}

#[cfg(capntproto_verus)]
verus! {
pub(super) closed spec fn selected(s: State, send: bool) -> u32 {
    match s { State::Sealed { readers, sends, .. } => if send { sends } else { readers }, _ => 0 }
}
pub(super) closed spec fn adjusted(s: State, t: State, send: bool, delta: int) -> bool {
    match (s, t) {
        (State::Sealed { length, readers, sends }, State::Sealed { length: n, readers: r, sends: w }) =>
            n == length && r as int == readers as int + (if send { 0int } else { delta })
                && w as int == sends as int + (if send { delta } else { 0int }),
        _ => false,
    }
}
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => requires s is Sealed, ensures match r {
    Ok(t) => selected(s, send) < u32::MAX && adjusted(s, t, send, 1),
    Err(e) => e == Error::ReferencesExhausted && selected(s, send) == u32::MAX,
}))]
pub(super) fn retain(s: State, send: bool) -> Result<State, Error> {
    let State::Sealed {
        length,
        readers,
        sends,
    } = s
    else {
        unreachable!()
    };
    Ok(if send {
        State::Sealed {
            length,
            readers,
            sends: sends.checked_add(1).ok_or(Error::ReferencesExhausted)?,
        }
    } else {
        State::Sealed {
            length,
            readers: readers.checked_add(1).ok_or(Error::ReferencesExhausted)?,
            sends,
        }
    })
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => requires s is Sealed, selected(s, send) > 0, ensures adjusted(s, r, send, -1)))]
pub(super) fn release(s: State, send: bool) -> State {
    let State::Sealed {
        length,
        readers,
        sends,
    } = s
    else {
        unreachable!()
    };
    if send {
        State::Sealed {
            length,
            readers,
            sends: sends - 1,
        }
    } else {
        State::Sealed {
            length,
            readers: readers - 1,
            sends,
        }
    }
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures r == match s {
    State::Sealed { readers, sends, .. } => readers == 0 && sends == 0,
    _ => false,
}))]
pub(super) fn reclaimable(s: State) -> bool {
    matches!(
        s,
        State::Sealed {
            readers: 0,
            sends: 0,
            ..
        }
    )
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => requires position as int + remaining as int <= usize::MAX, ensures
    match r {
        Ok((next, complete)) => count <= remaining && (count > 0 || remaining == 0)
            && next as int == position as int + count as int && complete == (count == remaining),
        Err(Error::InvalidWriteCount) => count > remaining,
        Err(Error::WriteZero) => count == 0 && remaining > 0,
        Err(_) => false,
    }
))]
pub(super) fn advance(
    position: usize,
    remaining: usize,
    count: usize,
) -> Result<(usize, bool), Error> {
    if count > remaining {
        return Err(Error::InvalidWriteCount);
    }
    if count == 0 && remaining != 0 {
        return Err(Error::WriteZero);
    }
    Ok((position + count, count == remaining))
}

/// The offset may name the next slot (one) even when the ring capacity is one.
#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => requires capacity > 0, head < capacity, offset <= capacity,
    ensures r < capacity, r as int ==
        if head as int + offset as int >= capacity { head as int + offset as int - capacity as int }
        else { head as int + offset as int }
))]
pub(super) fn ring_offset(head: usize, offset: usize, capacity: usize) -> usize {
    let until_wrap = capacity - head;
    if offset >= until_wrap {
        offset - until_wrap
    } else {
        head + offset
    }
}
