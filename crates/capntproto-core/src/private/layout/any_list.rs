//! Checked layout operations for schema-free list views.
use super::*;

impl<'a> PointerReader<'a> {
    pub(crate) fn get_any_list(self, default: Option<&'a [crate::Word]>) -> Result<ListReader<'a>> {
        self.get_list_any_size(default.map_or(ptr::null(), |words| words.as_ptr().cast()))
    }
}

impl<'a> PointerBuilder<'a> {
    pub(crate) fn get_any_list(
        self,
        default: Option<&'a [crate::Word]>,
    ) -> Result<ListBuilder<'a>> {
        let size = self.as_reader().get_any_list(default)?.element_size;
        if size == ElementSize::InlineComposite {
            self.get_struct_list(
                StructSize {
                    data: 0,
                    pointers: 0,
                },
                default,
            )
        } else {
            self.get_list(size, default)
        }
    }
}

impl<'a> ListReader<'a> {
    pub(crate) fn new_default_struct_list() -> Self {
        Self {
            element_size: ElementSize::InlineComposite,
            ..Self::new_default()
        }
    }

    pub(crate) fn check_element_size(&self, expected: ElementSize) -> Result<()> {
        // The null/default view has no elements and therefore no accesses.
        if self.element_count == 0 && self.element_size == ElementSize::Void {
            return Ok(());
        }
        // Bit access uses packed indexing, not a byte stride. Neither direction
        // of a bit/non-bit reinterpretation is a valid safe view.
        if (self.element_size == ElementSize::Bit) != (expected == ElementSize::Bit)
            || self.struct_data_size < data_bits_per_element(expected)
            || u32::from(self.struct_pointer_count) < pointers_per_element(expected)
        {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        Ok(())
    }

    pub(crate) fn check_struct_nesting(&self) -> Result<()> {
        if self.element_count > 0 && self.nesting_limit <= 0 {
            Err(Error::from_kind(ErrorKind::NestingLimitExceeded))
        } else {
            Ok(())
        }
    }

    pub(crate) fn get_struct_element_checked(&self, index: u32) -> Result<StructReader<'a>> {
        self.check_struct_nesting()?;
        Ok(self.get_struct_element(index))
    }

    pub(crate) fn data_only_bytes(self) -> Result<&'a [u8]> {
        if self.struct_pointer_count != 0 {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        if self.step == 0 {
            return Ok(&[]);
        }
        Ok(self.into_raw_bytes())
    }

    pub(crate) fn total_size(&self) -> Result<MessageSize> {
        let mut result = MessageSize {
            word_count: u64::from(wire_helpers::round_bits_up_to_words(
                u64::from(self.element_count) * u64::from(self.step),
            )) + u64::from(self.element_size == ElementSize::InlineComposite),
            cap_count: 0,
        };
        if self.struct_pointer_count > 0 {
            for i in 0..self.element_count {
                let offset = (u64::from(i) * u64::from(self.step)
                    + u64::from(self.struct_data_size))
                    / BITS_PER_BYTE as u64;
                let offset = usize::try_from(offset)
                    .map_err(|_| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
                // SAFETY: construction validated the physical list extent; i is
                // below element_count and this offset selects its pointer section.
                let pointers = unsafe { self.ptr.add(offset).cast::<WirePointer>() };
                for j in 0..self.struct_pointer_count {
                    // SAFETY: j is within the validated pointer section. The
                    // recursive traversal uses the same arena and nesting limit.
                    result += unsafe {
                        wire_helpers::total_size(
                            self.arena,
                            self.segment_id,
                            pointers.add(usize::from(j)),
                            self.nesting_limit,
                        )?
                    };
                }
            }
        }
        // Like StructReader::total_size, Rust charges child traversal reads.
        // C++ refunds this accounting; neither implementation ignores limits.
        Ok(result)
    }
}

impl ListBuilder<'_> {
    pub(crate) fn check_struct_size(&self, size: StructSize) -> Result<()> {
        if self.struct_data_size < u32::from(size.data) * u32::try_from(BITS_PER_WORD).unwrap()
            || self.struct_pointer_count < size.pointers
        {
            Err(Error::from_kind(ErrorKind::TypeMismatch))
        } else {
            Ok(())
        }
    }
}
