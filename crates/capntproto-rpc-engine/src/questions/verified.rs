verus! {
closed spec fn question_reclaimable(s: Record) -> bool {
    s.references == 0 && s.owner.is_none() && s.finish.is_none() && !question::awaiting_spec(s.lifecycle)
}
closed spec fn question_pipeline(s: Record) -> bool {
    s.published && s.references != 0 && !s.provision && question::pipeline_spec(s.lifecycle)
}
closed spec fn question_retained(s: Record, t: Record, r: Result<(), Error>) -> bool {
    if s.references == u32::MAX { r == Err(Error::ReferenceLimit) && t == s }
    else { r == Ok(()) && t == (Record { references: (s.references + 1) as u32, ..s }) }
}
proof fn cleanup_waits_for_return_and_accepted_finish(s: Record)
    requires s.finish.is_some() || question::awaiting_spec(s.lifecycle),
    ensures !question_reclaimable(s),
{}
}
