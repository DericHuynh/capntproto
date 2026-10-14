//! Wire capability-reference accounting, independent of local ownership and IO.
//!
//! Counts represent descriptors sent/received, not application handle clones.
//! The caller owns the capability slots, connection epochs and local lifetimes.
//! Retire an export only after `release` returns `true`. Retain import cleanup
//! state until its release ticket is accepted, or the connection is terminal.
use core::fmt;
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("references/verified.rs");

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Overflow,
    OverRelease,
    Released,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overflow => "capability wire reference count exhausted",
            Self::OverRelease => "release exceeds outstanding capability references",
            Self::Released => "export has already been released",
        })
    }
}
impl core::error::Error for Error {}

/// References granted to the peer. Zero is terminal, preventing resurrection
/// before the caller retires the slot. Failed operations preserve the count.
#[derive(Debug)]
#[cfg_attr(capntproto_verus, verus_verify)]
pub struct ExportRefs {
    count: u32,
}
impl Default for ExportRefs {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg_attr(capntproto_verus, verus_spec(r => ensures export_count(r) == 1))]
const fn initial_export() -> ExportRefs {
    ExportRefs { count: 1 }
}
#[inline]
#[cfg_attr(capntproto_verus, verus_spec(r => ensures export_count(r) == count))]
pub(crate) fn published_refs(count: u32) -> ExportRefs {
    ExportRefs { count }
}
impl ExportRefs {
    /// The first descriptor grants one reference.
    pub const fn new() -> Self {
        initial_export()
    }
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == export_count(*self)))]
    pub const fn count(&self) -> u32 {
        self.count
    }

    #[cfg_attr(capntproto_verus, verus_spec(r => ensures export_retain(*old(self), *final(self), count, r)))]
    pub fn retain(&mut self, count: u32) -> Result<(), Error> {
        if self.count == 0 {
            return Err(Error::Released);
        }
        self.count = self.count.checked_add(count).ok_or(Error::Overflow)?;
        Ok(())
    }

    /// Apply a validated peer Release or roll back an unpublished descriptor.
    /// Returns whether the caller must detach the capability owner and retire
    /// this export. Run destructors after releasing table/storage borrows.
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures export_release(*old(self), *final(self), count, r)))]
    pub fn release(&mut self, count: u32) -> Result<bool, Error> {
        if self.count == 0 {
            return Err(Error::Released);
        }
        self.count = self.count.checked_sub(count).ok_or(Error::OverRelease)?;
        Ok(self.count == 0)
    }
}

/// Descriptors received from the peer. Local retain/release policy is separate:
/// request a ticket only once no local owner needs these remote references.
#[derive(Debug, Default)]
#[cfg_attr(capntproto_verus, verus_verify)]
pub struct ImportRefs {
    count: u32,
}
#[cfg_attr(capntproto_verus, verus_spec(r => ensures import_count(r) == 0))]
const fn initial_import() -> ImportRefs {
    ImportRefs { count: 0 }
}
impl ImportRefs {
    pub const fn new() -> Self {
        initial_import()
    }
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == import_count(*self)))]
    pub const fn count(&self) -> u32 {
        self.count
    }

    #[cfg_attr(capntproto_verus, verus_spec(r => ensures import_receive(*old(self), *final(self), count, r)))]
    pub fn receive(&mut self, count: u32) -> Result<(), Error> {
        self.count = self.count.checked_add(count).ok_or(Error::Overflow)?;
        Ok(())
    }

    /// Borrow a pending Release effect. Dropping an unaccepted ticket changes
    /// nothing; output backpressure cannot silently forget the release. The
    /// exclusive borrow prevents concurrent receives/duplicate acknowledgments.
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures
        r.is_some() == (import_count(*old(self)) != 0),
        match r {
            Some(ticket) => ticket_count(ticket) == import_count(*old(self))
                && import_count(*final(self)) == ticket_final_count(ticket),
            None => *final(self) == *old(self),
        }
    ))]
    pub fn pending_release(&mut self) -> Option<ReleaseTicket<'_>> {
        if self.count == 0 {
            None
        } else {
            Some(ReleaseTicket { refs: self })
        }
    }
}

/// A Release effect borrowed until an output adapter accepts it. Acceptance is
/// transfer of message ownership, not a flush, peer receipt or RPC completion.
/// The caller must reserve control-message capacity or terminate the connection
/// if cleanup cannot progress. This ticket itself never allocates or queues IO.
#[must_use = "retain the ticket until output accepts it; dropping it preserves pending cleanup"]
#[cfg_attr(capntproto_verus, verus_verify)]
pub struct ReleaseTicket<'a> {
    refs: &'a mut ImportRefs,
}
impl ReleaseTicket<'_> {
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == ticket_count(*self)))]
    pub const fn count(&self) -> u32 {
        self.refs.count
    }
    #[cfg_attr(capntproto_verus, verus_spec(ensures ticket_final_count(self) == 0))]
    pub fn commit(self) {
        self.refs.count = 0;
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
