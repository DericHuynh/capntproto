//! Explicit index translation for copying between RPC payloads.
use crate::{Error, ErrorKind, Result};

/// Validated destination indices. The connection must separately bind both
/// descriptor tables to their grants; this object does not create authority.
#[derive(Clone, Copy, Debug)]
pub struct IndexMap<'a> {
    indices: Option<&'a [u32]>,
    count: u32,
}
impl<'a> IndexMap<'a> {
    pub fn new(indices: &'a [u32], destination_count: u32) -> Result<Self> {
        if indices.iter().any(|&i| i >= destination_count) {
            return Err(Error::from_kind(
                ErrorKind::MessageContainsInvalidCapabilityPointer,
            ));
        }
        Ok(Self {
            indices: Some(indices),
            count: destination_count,
        })
    }
    /// Preserve indices when the caller retains the complete descriptor table
    /// in order, including null slots. Bounds are still checked on every copy.
    /// This avoids materializing an identity table in caller memory.
    pub const fn identity(count: u32) -> Self {
        Self {
            indices: None,
            count,
        }
    }
    pub(crate) fn remap(self, source: u32) -> Result<u32> {
        let Some(indices) = self.indices else {
            return if source < self.count {
                Ok(source)
            } else {
                Err(Error::from_kind(
                    ErrorKind::MessageContainsInvalidCapabilityPointer,
                ))
            };
        };
        usize::try_from(source)
            .ok()
            .and_then(|index| indices.get(index))
            .copied()
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageContainsInvalidCapabilityPointer))
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs {
    use super::*;
    #[kani::proof]
    fn identity_mapping_is_exact_and_bounded() {
        let count: u32 = kani::any();
        let source: u32 = kani::any();
        match IndexMap::identity(count).remap(source) {
            Ok(mapped) => {
                assert!(source < count);
                assert_eq!(mapped, source);
            }
            Err(error) => {
                assert!(source >= count);
                assert_eq!(
                    error.kind,
                    ErrorKind::MessageContainsInvalidCapabilityPointer
                );
            }
        }
    }
}
