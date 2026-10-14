// Inductive record transitions preserve owner, identity, routing and publication
// metadata. Table lookup separately checks the retained generation.
verus! {
closed spec fn reclaimable<T>(s: Record<T>) -> bool {
    s.finished && s.state == State::Returned && s.pipelines == 0
}
closed spec fn wire_live<T>(s: Record<T>) -> bool {
    !(s.finished && s.state == State::Returned)
}
closed spec fn cancel_requested<T>(s: Record<T>) -> bool {
    s.finished && s.pipelines == 0 && (s.state == State::Queued || s.state == State::Running { cancellation: true })
}
closed spec fn retained<T>(s: Record<T>, t: Record<T>, r: Result<(), Error>) -> bool {
    if s.pipelines == u32::MAX { r == Err(Error::ReferenceLimit) && t == s }
    else { r == Ok(()) && t == (Record { pipelines: (s.pipelines + 1) as u32, ..s }) }
}
closed spec fn dispatched<T>(s: Record<T>, t: Record<T>, cancellation: bool, r: Result<bool, Error>) -> bool {
    if s.state != State::Queued { r == Err(Error::AlreadyDispatched) && t == s }
    else { r == Ok(!s.finished || s.pipelines != 0) && t == (Record { state: State::Running { cancellation }, ..s }) }
}
closed spec fn finished<T>(s: Record<T>, t: Record<T>, release: bool, r: Option<(bool, bool)>) -> bool {
    if s.finished { r == None && t == s }
    else { t == (Record { finished: true, ..s }) && r == Some((cancel_requested(t), release && !s.provision)) }
}
closed spec fn adoption_result<T>(s: Record<T>) -> Result<(), Error> {
    match s.redirect {
        Redirect::None | Redirect::ThirdParty => Err(Error::NotRedirected),
        Redirect::Adopted => Err(Error::AlreadyAdopted),
        Redirect::Available => if s.pipelines == u32::MAX { Err(Error::ReferenceLimit) } else { Ok(()) },
    }
}

/// Finish and Return cannot make a retained producer reclaimable, regardless of
/// their ordering; retaining a producer leaves its identity and value unchanged.
proof fn retained_producer_survives_finish<T>(s: Record<T>, t: Record<T>, release: bool, r: Option<(bool, bool)>)
    requires s.pipelines > 0, finished(s, t, release, r),
    ensures !reclaimable(t), t.value == s.value, t.generation == s.generation, t.id == s.id,
{}

/// The same executable eligibility check used by direct wire publication
/// guarantees that accepting an omitted-Finish Return can reclaim its producer.
fn retire_direct_return<T>(record: &mut Record<T>, omission_allowed: bool)
    requires omission_allowed, old(record).pipelines == 0,
    ensures reclaimable(*final(record)), final(record).value == old(record).value,
        final(record).generation == old(record).generation, final(record).id == old(record).id,
{
    if record.can_retire_on_return(omission_allowed) {
        record.accept_return(true);
    }
}
}
