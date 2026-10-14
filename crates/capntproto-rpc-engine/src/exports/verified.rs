verus! {
closed spec fn wire_count(s: State) -> u32 {
    match s { State::Live(r) => crate::references::export_count(r), _ => 0 }
}
closed spec fn valid_entry<T>(s: Entry<T>) -> bool {
    wire_count(s.state) as int + s.pending as int <= u32::MAX
        && (s.state is Live ==> wire_count(s.state) > 0)
        && (s.state is Staged ==> s.pending > 0)
        && (s.state is Retired ==> s.pending == 0)
}
closed spec fn aborted<T>(s: Entry<T>, t: Entry<T>) -> bool {
    t == (Entry { state: if s.state is Staged { State::Retired } else { s.state }, pending: 0, ..s })
}
closed spec fn reserved<T>(s: Entry<T>, t: Entry<T>, r: Result<(), Error>) -> bool {
    if s.promise == promise::ImportState::Resolved { r == Err(Error::Promise(promise::Error::AlreadyResolved)) && t == s }
    else if s.state is Retired { r == Err(Error::StaleKey) && t == s }
    else if wire_count(s.state) as int + s.pending as int + 1 > u32::MAX {
        r == Err(Error::RemoteReferencesExhausted) && t == s
    } else { r == Ok(()) && t == (Entry { pending: (s.pending + 1) as u32, ..s }) }
}
closed spec fn committed<T>(s: Entry<T>, t: Entry<T>) -> bool {
    t.pending == 0 && t.value == s.value && t.local == s.local && t.promise == s.promise
        && (if s.state is Retired { t.state is Retired }
            else { t.state is Live && wire_count(t.state) as int == wire_count(s.state) as int + s.pending as int })
}
proof fn accepted_reservations_preserve_bounds<T>(s: Entry<T>, t: Entry<T>, r: Result<(), Error>)
    requires valid_entry(s), reserved(s, t, r),
    ensures valid_entry(t),
{}
proof fn unaccepted_grants_do_not_publish<T>(s: Entry<T>, t: Entry<T>)
    requires aborted(s, t),
    ensures wire_count(t.state) == wire_count(s.state), t.value == s.value, t.local == s.local,
{}
}
