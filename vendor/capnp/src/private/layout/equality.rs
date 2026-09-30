//! Structural comparison, corresponding to capnp/any.c++.
//!
//! Traverse through the ordinary readers so their bounds, traversal and nesting
//! limits remain in force. In particular, do not short-circuit identical reader
//! addresses: a value containing capabilities is unknown even against itself.

use super::{ElementSize, ListReader, PointerReader, PointerType, StructReader};
use crate::{Equality, Error, ErrorKind, Result};

impl PointerReader<'_> {
    pub(crate) fn equals(&self, right: PointerReader<'_>) -> Result<Equality> {
        use PointerType::*;
        match (self.get_pointer_type()?, right.get_pointer_type()?) {
            (Null, Null) => Ok(Equality::Equal),
            (Struct, Struct) => self.get_struct(None)?.equals(right.get_struct(None)?),
            (List, List) => self
                .get_list_any_size(core::ptr::null())?
                .equals(right.get_list_any_size(core::ptr::null())?),
            (Capability, Capability) => Ok(Equality::UnknownContainsCapabilities),
            _ => Ok(Equality::NotEqual),
        }
    }
}

fn without_trailing_zeroes(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |i| i + 1);
    &bytes[..end]
}

impl StructReader<'_> {
    pub(crate) fn equals(&self, right: StructReader<'_>) -> Result<Equality> {
        if without_trailing_zeroes(self.get_data_section_as_blob())
            != without_trailing_zeroes(right.get_data_section_as_blob())
        {
            return Ok(Equality::NotEqual);
        }

        let pointer_count = |reader: &StructReader<'_>| {
            let mut count = usize::from(reader.get_pointer_section_size());
            while count > 0 && reader.get_pointer_field(count - 1).is_null() {
                count -= 1;
            }
            count
        };
        let count = pointer_count(self);
        if count != pointer_count(&right) {
            return Ok(Equality::NotEqual);
        }

        let mut result = Equality::Equal;
        for index in 0..count {
            match self
                .get_pointer_field(index)
                .equals(right.get_pointer_field(index))?
            {
                Equality::NotEqual => return Ok(Equality::NotEqual),
                Equality::UnknownContainsCapabilities => {
                    result = Equality::UnknownContainsCapabilities;
                }
                Equality::Equal => {}
            }
        }
        Ok(result)
    }
}

impl ListReader<'_> {
    pub(crate) fn equals(&self, right: ListReader<'_>) -> Result<Equality> {
        if self.len() != right.len() || self.get_element_size() != right.get_element_size() {
            return Ok(Equality::NotEqual);
        }

        match self.get_element_size() {
            ElementSize::Pointer | ElementSize::InlineComposite => {
                // C++ visits pointer-list elements as single-pointer structs.
                // Match that extra nesting level, including empty structs.
                if !self.is_empty() && (self.nesting_limit <= 0 || right.nesting_limit <= 0) {
                    return Err(Error::from_kind(ErrorKind::NestingLimitExceeded));
                }
                let mut result = Equality::Equal;
                for index in 0..self.len() {
                    match self
                        .get_struct_element(index)
                        .equals(right.get_struct_element(index))?
                    {
                        Equality::NotEqual => return Ok(Equality::NotEqual),
                        Equality::UnknownContainsCapabilities => {
                            result = Equality::UnknownContainsCapabilities;
                        }
                        Equality::Equal => {}
                    }
                }
                Ok(result)
            }
            _ => {
                let left = self.into_raw_bytes();
                let right = right.into_raw_bytes();
                let mut size = left.len();
                if self.get_element_size() == ElementSize::Bit && !self.len().is_multiple_of(8) {
                    size -= 1;
                    let mask = (1u8 << (self.len() % 8)) - 1;
                    if left[size] & mask != right[size] & mask {
                        return Ok(Equality::NotEqual);
                    }
                }
                Ok(if left[..size] == right[..size] {
                    Equality::Equal
                } else {
                    Equality::NotEqual
                })
            }
        }
    }
}
