verus! {
closed spec fn received<T>(s: Slot<T>, t: Slot<T>, r: Result<bool, Error>) -> bool {
    let e = s.entry.unwrap();
    if e.local == u32::MAX { r == Err(Error::LocalReferencesExhausted) && t == s }
    else if e.local == 0 && s.generation == u64::MAX { r == Err(Error::GenerationExhausted) && t == s }
    else if crate::references::import_count(e.remote) == u32::MAX { r == Err(Error::RemoteReferencesExhausted) && t == s }
    else {
        &&& r == Ok(e.local == 0)
        &&& t.generation as int == s.generation as int + (if e.local == 0 { 1int } else { 0int })
        &&& t.entry.is_some()
        &&& t.entry.unwrap().local as int == e.local as int + 1
        &&& crate::references::import_count(t.entry.unwrap().remote) as int == crate::references::import_count(e.remote) as int + 1
        &&& t.entry.unwrap().value == e.value
        &&& t.entry.unwrap().wire == e.wire
        &&& t.entry.unwrap().promise == e.promise
        &&& t.entry.unwrap().client_binding == (if e.local == 0 { None } else { e.client_binding })
    }
}
proof fn revived_import_cannot_reuse_stale_generation<T>(s: Slot<T>, t: Slot<T>)
    requires s.entry.is_some(), s.entry.unwrap().local == 0, received(s, t, Ok(true)),
    ensures t.generation > s.generation, t.entry.unwrap().value == s.entry.unwrap().value,
{}
}
