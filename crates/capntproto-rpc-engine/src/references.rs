//! Wire capability-reference accounting, independent of local ownership and IO.
//!
//! Counts represent descriptors sent/received, not application handle clones.
//! The caller owns the capability slots, connection epochs and local lifetimes.
//! Retire an export only after `release` returns `true`. Retain import cleanup
//! state until its release ticket is accepted, or the connection is terminal.
use core::fmt;

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
pub struct ExportRefs {
    count: u32,
}
impl Default for ExportRefs {
    fn default() -> Self {
        Self::new()
    }
}
impl ExportRefs {
    /// The first descriptor grants one reference.
    pub const fn new() -> Self {
        Self { count: 1 }
    }
    pub const fn count(&self) -> u32 {
        self.count
    }

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
pub struct ImportRefs {
    count: u32,
}
impl ImportRefs {
    pub const fn new() -> Self {
        Self { count: 0 }
    }
    pub const fn count(&self) -> u32 {
        self.count
    }

    pub fn receive(&mut self, count: u32) -> Result<(), Error> {
        self.count = self.count.checked_add(count).ok_or(Error::Overflow)?;
        Ok(())
    }

    /// Borrow a pending Release effect. Dropping an unaccepted ticket changes
    /// nothing; output backpressure cannot silently forget the release. The
    /// exclusive borrow prevents concurrent receives/duplicate acknowledgments.
    pub fn pending_release(&mut self) -> Option<ReleaseTicket<'_>> {
        (self.count != 0).then_some(ReleaseTicket { refs: self })
    }
}

/// A Release effect borrowed until an output adapter accepts it. Acceptance is
/// transfer of message ownership, not a flush, peer receipt or RPC completion.
/// The caller must reserve control-message capacity or terminate the connection
/// if cleanup cannot progress. This ticket itself never allocates or queues IO.
#[must_use = "retain the ticket until output accepts it; dropping it preserves pending cleanup"]
pub struct ReleaseTicket<'a> {
    refs: &'a mut ImportRefs,
}
impl ReleaseTicket<'_> {
    pub const fn count(&self) -> u32 {
        self.refs.count
    }
    pub fn commit(self) {
        self.refs.count = 0;
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
