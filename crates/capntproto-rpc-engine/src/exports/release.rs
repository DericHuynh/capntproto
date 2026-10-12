//! Atomic implicit releases for Return.releaseParamCaps and Finish.releaseResultCaps.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseError {
    ScratchTooSmall,
    Export(Error),
    Rejected,
}
impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "export release batch: {self:?}")
    }
}
impl core::error::Error for ReleaseError {}
impl<'id, 'storage, T> Exports<'id, 'storage, T> {
    /// Collect releases from one previously accepted message. No count changes
    /// until the complete message's descriptors have been checked and accepted.
    pub fn prepare_releases<'a, 'scratch>(
        &'a mut self,
        scratch: &'scratch mut [u32],
    ) -> Result<ReleaseBatch<'a, 'scratch, 'id, 'storage, T>, ReleaseError> {
        self.abort_pending();
        let counts = scratch
            .get_mut(..self.slots.len())
            .ok_or(ReleaseError::ScratchTooSmall)?;
        counts.fill(0);
        Ok(ReleaseBatch {
            exports: self,
            counts,
            rejected: false,
        })
    }
}
#[must_use]
pub struct ReleaseBatch<'a, 'scratch, 'id, 'storage, T> {
    exports: &'a mut Exports<'id, 'storage, T>,
    counts: &'scratch mut [u32],
    rejected: bool,
}
impl<'a, 'scratch, 'id, 'storage, T> ReleaseBatch<'a, 'scratch, 'id, 'storage, T> {
    /// Ignoring a failed record cannot authorize a partially accepted batch.
    pub fn record(&mut self, id: ExportId, count: u32) -> Result<(), ReleaseError> {
        let result = self.record_inner(id, count);
        self.rejected |= result.is_err();
        result
    }
    fn record_inner(&mut self, id: ExportId, count: u32) -> Result<(), ReleaseError> {
        if self.rejected {
            return Err(ReleaseError::Rejected);
        }
        let index = self
            .exports
            .live_index(id)
            .ok_or(ReleaseError::Export(Error::Missing))?;
        let State::Live(refs) = &self.exports.slots[index].entry.as_ref().unwrap().state else {
            unreachable!()
        };
        let total = self.counts[index]
            .checked_add(count)
            .filter(|&n| n <= refs.count())
            .ok_or(ReleaseError::Export(Error::OverRelease))?;
        self.counts[index] = total;
        Ok(())
    }
    pub fn finish(self) -> Result<Releases<'a, 'scratch, 'id, 'storage, T>, ReleaseError> {
        if self.rejected {
            return Err(ReleaseError::Rejected);
        }
        Ok(Releases {
            exports: self.exports,
            counts: self.counts,
        })
    }
}
/// Counts are checked and immutable until acceptance. Retired owners remain in
/// slots for explicit draining, so commit invokes no destructors or callbacks.
#[must_use]
pub struct Releases<'a, 'scratch, 'id, 'storage, T> {
    exports: &'a mut Exports<'id, 'storage, T>,
    counts: &'scratch [u32],
}
impl<'a, 'scratch, 'id, 'storage, T> Releases<'a, 'scratch, 'id, 'storage, T> {
    pub fn accept(self) {
        apply(self.exports, self.counts);
    }
    /// A Return may release its original parameter grants while handing a local
    /// capability back to its owner. Acquire every local lease in the same commit.
    pub fn with_leases<'r>(
        self,
        receipts: &'r mut [LeaseSlot<'id>],
    ) -> Result<LeasedReleases<'a, 'scratch, 'r, 'id, 'storage, T>, LeaseError> {
        Ok(LeasedReleases {
            leases: self.exports.prepare_leases(receipts)?,
            counts: self.counts,
        })
    }
}
#[must_use]
pub struct LeasedReleases<'a, 'scratch, 'r, 'id, 'storage, T> {
    leases: Leases<'a, 'r, 'id, 'storage, T>,
    counts: &'scratch [u32],
}
impl<'r, 'id, T> LeasedReleases<'_, '_, 'r, 'id, '_, T> {
    pub fn accept(mut self) -> &'r mut [LeaseSlot<'id>] {
        self.leases.commit();
        apply(self.leases.exports, self.counts);
        core::mem::take(&mut self.leases.receipts)
    }
}
fn apply<T>(exports: &mut Exports<'_, '_, T>, counts: &[u32]) {
    for (slot, &count) in exports.slots.iter_mut().zip(counts) {
        if count == 0 {
            continue;
        }
        let entry = slot.entry.as_mut().expect("preflight release");
        let State::Live(refs) = &mut entry.state else {
            unreachable!()
        };
        if refs.release(count).expect("preflight reference count") {
            entry.state = State::Retired;
        }
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
