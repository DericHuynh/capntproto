//! Reply ownership transitions, shared by fixed and heap-owned endpoints.
use super::Error;
#[cfg(capntproto_verus)]
use vstd::prelude::*;

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures r == *old(value), *final(value) == next))]
fn replace<T>(value: &mut T, mut next: T) -> T {
    core::mem::swap(value, &mut next);
    next
}

#[cfg_attr(capntproto_verus, verus_verify)]
pub(super) enum Phase<T> {
    Idle,
    Pending,
    Value(T),
    Abandoned(T),
    Canceled,
    Closed,
    Consumed,
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures
    if *old(phase) is Pending { r == Ok(()) && *final(phase) == Phase::Value(value) }
    else { r == Err(value) && *final(phase) == *old(phase) }
))]
pub(super) fn send<T>(phase: &mut Phase<T>, value: T) -> Result<(), T> {
    if !matches!(phase, Phase::Pending) {
        return Err(value);
    }
    *phase = Phase::Value(value);
    Ok(())
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures r == !(*phase is Pending || *phase is Value)))]
pub(super) fn observed<T>(phase: &Phase<T>) -> bool {
    !matches!(phase, Phase::Pending | Phase::Value(_))
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match *old(phase) {
    Phase::Pending => r == None && *final(phase) == *old(phase),
    Phase::Value(value) => r == Some(Ok(value)) && *final(phase) == Phase::Consumed,
    Phase::Canceled => r == Some(Err(Error::Canceled)) && *final(phase) == Phase::Consumed,
    _ => r == Some(Err(Error::Consumed)) && *final(phase) == *old(phase),
}))]
pub(super) fn take<T>(phase: &mut Phase<T>) -> Option<Result<T, Error>> {
    if matches!(phase, Phase::Pending) {
        return None;
    }
    if !matches!(phase, Phase::Value(_) | Phase::Canceled) {
        return Some(Err(Error::Consumed));
    }
    Some(match replace(phase, Phase::Consumed) {
        Phase::Value(value) => Ok(value),
        Phase::Canceled => Err(Error::Canceled),
        _ => Err(Error::Consumed),
    })
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(ensures match *old(phase) {
    Phase::Value(value) | Phase::Abandoned(value) => *final(phase) == Phase::Abandoned(value),
    _ => *final(phase) == Phase::Closed,
}))]
pub(super) fn abandon<T>(phase: &mut Phase<T>) {
    if matches!(phase, Phase::Value(_)) {
        if let Phase::Value(value) = replace(phase, Phase::Closed) {
            *phase = Phase::Abandoned(value);
        }
    } else if !matches!(phase, Phase::Abandoned(_)) {
        *phase = Phase::Closed;
    }
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match *old(phase) {
    Phase::Value(value) | Phase::Abandoned(value) => r == Some(value) && *final(phase) == Phase::Canceled,
    Phase::Pending => r == None && *final(phase) == Phase::Canceled,
    _ => r == None && *final(phase) == *old(phase),
}))]
pub(super) fn cancel<T>(phase: &mut Phase<T>) -> Option<T> {
    match phase {
        Phase::Pending | Phase::Value(_) | Phase::Abandoned(_) => {
            match replace(phase, Phase::Canceled) {
                Phase::Value(value) | Phase::Abandoned(value) => Some(value),
                _ => None,
            }
        }
        _ => None,
    }
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures match *old(phase) {
    Phase::Abandoned(value) => r == Some(value) && *final(phase) == Phase::Closed,
    _ => r == None && *final(phase) == *old(phase),
}))]
pub(super) fn take_abandoned<T>(phase: &mut Phase<T>) -> Option<T> {
    if !matches!(phase, Phase::Abandoned(_)) {
        return None;
    }
    let Phase::Abandoned(value) = replace(phase, Phase::Closed) else {
        unreachable!()
    };
    Some(value)
}

#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (endpoints == 0)))]
pub(super) fn can_claim(endpoints: u8) -> bool {
    endpoints == 0
}
