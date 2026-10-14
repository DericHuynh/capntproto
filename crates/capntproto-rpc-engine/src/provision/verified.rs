verus! {
pub closed spec fn embargo_initial(s: EmbargoState) -> bool {
    !s.claimed && !s.released && !s.closed
}
pub closed spec fn embargo_claim(s: EmbargoState, t: EmbargoState, r: Result<(), Error>) -> bool {
    if s.closed { r == Err(Error::Closed) && t == s }
    else if s.claimed { r == Err(Error::DuplicateAccept) && t == s }
    else { r == Ok(()) && t == (EmbargoState { claimed: true, ..s }) }
}
pub closed spec fn embargo_release(s: EmbargoState, t: EmbargoState, r: Result<(), Error>) -> bool {
    if s.closed { r == Err(Error::Closed) && t == s }
    else if s.released { r == Err(Error::DuplicateRelease) && t == s }
    else { r == Ok(()) && t == (EmbargoState { released: true, ..s }) }
}
pub closed spec fn embargo_close(s: EmbargoState, t: EmbargoState) -> bool {
    t == (EmbargoState { closed: true, ..s })
}
pub closed spec fn embargo_readiness(s: EmbargoState) -> Result<(), Error> {
    if !s.claimed { Err(Error::Pending) }
    else if s.released { Ok(()) }
    else if s.closed { Err(Error::Closed) }
    else { Err(Error::Pending) }
}
}
