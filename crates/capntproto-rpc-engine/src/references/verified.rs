// These contracts verify the production bodies for every u32 count. A release
// ticket's mutable borrow links its eventual count back to the original owner.
verus! {
pub closed spec fn export_count(s: ExportRefs) -> u32 { s.count }
pub closed spec fn import_count(s: ImportRefs) -> u32 { s.count }
pub closed spec fn ticket_count(s: ReleaseTicket) -> u32 { s.refs.count }
#[verifier::prophetic]
pub closed spec fn ticket_final_count(s: ReleaseTicket) -> u32 { final(s.refs).count }

pub open spec fn export_retain(s: ExportRefs, t: ExportRefs, n: u32, r: Result<(), Error>) -> bool {
    if export_count(s) == 0 { r == Err(Error::Released) && t == s }
    else if export_count(s) as int + n as int > u32::MAX { r == Err(Error::Overflow) && t == s }
    else { r == Ok(()) && export_count(t) as int == export_count(s) as int + n as int }
}
pub closed spec fn export_release(s: ExportRefs, t: ExportRefs, n: u32, r: Result<bool, Error>) -> bool {
    if s.count == 0 { r == Err(Error::Released) && t == s }
    else if n > s.count { r == Err(Error::OverRelease) && t == s }
    else { r == Ok(n == s.count) && t.count as int == s.count as int - n as int }
}
pub open spec fn import_receive(s: ImportRefs, t: ImportRefs, n: u32, r: Result<(), Error>) -> bool {
    if import_count(s) as int + n as int > u32::MAX { r == Err(Error::Overflow) && t == s }
    else { r == Ok(()) && import_count(t) as int == import_count(s) as int + n as int }
}
}
