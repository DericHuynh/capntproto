//! Exact descriptor multiplicity checks, using caller scratch instead of a map.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorError {
    ScratchTooSmall,
    Unreserved,
    Missing,
    PromiseKind,
}
impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ScratchTooSmall => "descriptor scratch is smaller than the export table",
            Self::Unreserved => "descriptor has no matching export reservation",
            Self::Missing => "reserved export is missing from the message",
            Self::PromiseKind => "descriptor promise kind does not match its reserved grant",
        })
    }
}
impl core::error::Error for DescriptorError {}

impl<T> Batch<'_, '_, '_, T> {
    /// Count the descriptors decoded from one sealed message. Scratch needs one
    /// u32 per export slot; only that prefix is cleared/used. No table state is
    /// changed. The wire adapter must retain the validated bytes until acceptance.
    pub fn descriptors<'a>(
        &'a self,
        scratch: &'a mut [u32],
    ) -> Result<DescriptorCheck<'a, T>, DescriptorError> {
        let counts = scratch
            .get_mut(..self.exports.slots.len())
            .ok_or(DescriptorError::ScratchTooSmall)?;
        counts.fill(0);
        Ok(DescriptorCheck {
            slots: &self.exports.slots,
            counts,
            rejected: false,
        })
    }
}

/// Dropping a check does not authorize publication. A rejected descriptor poisons
/// the check, so ignoring its error cannot turn a mismatched frame into success.
#[must_use = "record every decoded descriptor and check finish before publication"]
pub struct DescriptorCheck<'a, T> {
    pub(super) slots: &'a [Slot<T>],
    pub(super) counts: &'a mut [u32],
    pub(super) rejected: bool,
}
impl<T> DescriptorCheck<'_, T> {
    pub fn record(&mut self, id: ExportId) -> Result<(), DescriptorError> {
        self.record_kind(id, false)
    }
    pub fn record_kind(&mut self, id: ExportId, promise: bool) -> Result<(), DescriptorError> {
        let Ok(index) = usize::try_from(id.to_wire()) else {
            self.rejected = true;
            return Err(DescriptorError::Unreserved);
        };
        let reserved = self
            .slots
            .get(index)
            .and_then(|s| s.entry.as_ref())
            .map_or(0, |e| e.pending);
        if self.rejected || reserved == 0 || self.counts[index] >= reserved {
            self.rejected = true;
            return Err(DescriptorError::Unreserved);
        }
        let entry = self.slots[index].entry.as_ref().unwrap();
        let pending = entry.promise == promise::ImportState::Pending;
        // The first descriptor for a new promise must introduce it. Subsequent
        // aliases (including later messages) may be senderHosted, as in legacy.
        if (promise && !pending)
            || (!promise
                && pending
                && matches!(entry.state, State::Staged)
                && self.counts[index] == 0)
        {
            self.rejected = true;
            return Err(DescriptorError::PromiseKind);
        }
        self.counts[index] += 1;
        Ok(())
    }
    pub fn finish(self) -> Result<(), DescriptorError> {
        if self.rejected {
            return Err(DescriptorError::Unreserved);
        }
        for (slot, count) in self.slots.iter().zip(self.counts.iter()) {
            let reserved = slot.entry.as_ref().map_or(0, |e| e.pending);
            if *count != reserved {
                return Err(DescriptorError::Missing);
            }
        }
        Ok(())
    }
}
