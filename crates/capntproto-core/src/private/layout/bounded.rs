//! Fallible layout transitions used by the restricted fixed-buffer editors.
#![deny(unsafe_op_in_unsafe_fn)]
use super::*;

impl PointerReader<'_> {
    pub(crate) fn validate_capability_indices(self, count: u32) -> Result<()> {
        if self.is_null() {
            return Ok(());
        }
        // SAFETY: a non-default PointerReader owns a validated pointer word;
        // is_null above also excludes the default reader's null address. Reading
        // its tag does not follow its target. get_struct/get_list below perform
        // the complete target, traversal-budget and nesting checks exactly once.
        let kind = match unsafe { (*self.pointer).kind() } {
            WirePointerKind::Struct => PointerType::Struct,
            WirePointerKind::List => PointerType::List,
            // Landing pads and unknown/capability tags retain the full checked
            // decoding path. No far target is read before validating its pad.
            _ => self.get_pointer_type()?,
        };
        match kind {
            PointerType::Null => (),
            PointerType::Capability => {
                if self.get_capability_index()? >= count {
                    return Err(Error::from_kind(
                        ErrorKind::MessageContainsInvalidCapabilityPointer,
                    ));
                }
            }
            PointerType::Struct => {
                let value = self.get_struct(None)?;
                for i in 0..usize::from(value.get_pointer_section_size()) {
                    value
                        .get_pointer_field(i)
                        .validate_capability_indices(count)?;
                }
            }
            PointerType::List => {
                let value = self.get_list_any_size(ptr::null())?;
                match value.element_size {
                    ElementSize::Pointer => {
                        for i in 0..value.len() {
                            value
                                .get_pointer_element(i)
                                .validate_capability_indices(count)?;
                        }
                    }
                    ElementSize::InlineComposite if value.struct_pointer_count != 0 => {
                        for i in 0..value.len() {
                            let element = value.get_struct_element(i);
                            for j in 0..usize::from(element.get_pointer_section_size()) {
                                element
                                    .get_pointer_field(j)
                                    .validate_capability_indices(count)?;
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }
}

impl<'a> PointerBuilder<'a> {
    pub(crate) fn copy_wire_from(
        &mut self,
        source: PointerReader<'_>,
        mapping: crate::wire_capability::IndexMap<'_>,
    ) -> Result<()> {
        let CapTableBuilder::Plain(table) = self.cap_table;
        if self.arena.allows_capabilities() || !table.is_null() {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        // A missing pointer field (including an untouched message root) has no
        // backing wire word. Ordinary copying already handles this default
        // reader without dereferencing its null pointer. It has no cap indices.
        if source.pointer.is_null() {
            return self.copy_from(source, false);
        }
        // SAFETY: the destination is an exclusively borrowed initialized slot
        // in a fixed arena. The source has a wire word and its reader arena
        // validates every traversal;
        // the mapper cannot invoke hooks and bounds every destination index.
        unsafe {
            wire_helpers::deep_copy_pointee(
                self.arena,
                self.segment_id,
                self.cap_table,
                self.pointer,
                source,
                false,
                Some(mapping),
            )?;
        }
        Ok(())
    }

    /// A fixed arena has no hook table. Its wire indices are owned by the RPC
    /// message descriptor table; clearing an index does not release that table.
    pub(crate) fn set_wire_capability(&mut self, index: u32) -> Result<()> {
        let CapTableBuilder::Plain(table) = self.cap_table;
        if self.arena.allows_capabilities() || !table.is_null() {
            return Err(Error::from_kind(ErrorKind::TypeMismatch));
        }
        // SAFETY: this builder exclusively owns its initialized pointer slot.
        // Fixed-arena indices have no hooks to drop, and zero_object clears the
        // old owned subtree before the immediate capability pointer replaces it.
        unsafe {
            wire_helpers::zero_object(self.arena, self.cap_table, self.segment_id, self.pointer);
            (*self.pointer).set_cap(index);
        }
        Ok(())
    }

    /// Promote a present list to an inline struct list without losing unknown
    /// data or duplicating descendants. Bit-list promotion is not wire-compatible.
    pub(crate) fn try_upgrade_struct_list(mut self, size: StructSize) -> Result<ListBuilder<'a>> {
        if self.is_null() {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        let reader = self.as_reader().get_list(InlineComposite, None)?;
        if reader.struct_layout_fits(size) {
            return self.get_struct_list(size, None);
        }
        // Void access acquires the physical layout without performing an upgrade
        // or copying defaults. The reader validation above rejects bit lists.
        let (old_ptr, old_segment, count, old_step, old_data, old_pointers) = {
            let old = self.reborrow().get_list(Void, None)?;
            (
                old.ptr,
                old.segment_id,
                old.element_count,
                old.step / 8,
                old.struct_data_size / 8,
                old.struct_pointer_count,
            )
        };
        let size = StructSize {
            data: size.data.max(u16::try_from(old_data.div_ceil(8)).unwrap()),
            pointers: size.pointers.max(old_pointers),
        };
        self.stage(|p| {
            let new = p.try_init_struct_list(count, size)?;
            // Empty/void elements need no iteration, even when their encoded
            // count is large. Their newly reserved contents are already zero.
            if old_data != 0 || old_pointers != 0 {
                // SAFETY: the old physical list is validated and exclusively
                // borrowed. The staged list is a disjoint initialized allocation
                // large enough for every original data byte and pointer. All
                // fallible reservations precede this transfer. Fixed arenas are
                // single-segment, so moving child pointers cannot reserve again.
                // Clear each source slot to give its payload exactly one owner.
                unsafe {
                    for i in 0..count as usize {
                        let src = old_ptr.add(i * old_step as usize);
                        let dst = new.ptr.add(i * (new.step / 8) as usize);
                        wire_helpers::copy_nonoverlapping_check_zero(src, dst, old_data as usize);
                        let src_pointers = src.add(old_data as usize).cast::<WirePointer>();
                        let dst_pointers =
                            dst.add(usize::from(size.data) * 8).cast::<WirePointer>();
                        for j in 0..usize::from(old_pointers) {
                            wire_helpers::transfer_pointer(
                                new.arena,
                                new.segment_id,
                                dst_pointers.add(j),
                                old_segment,
                                src_pointers.add(j),
                            );
                            ptr::write_bytes(src_pointers.add(j), 0, 1);
                        }
                    }
                }
            }
            Ok(())
        })?;
        self.get_struct_list(size, None)
    }
}

#[cfg(test)]
mod validation_tests {
    use crate::{
        any_pointer,
        message::{Reader, ReaderOptions, SegmentArray},
        word, Word,
    };

    #[test]
    fn capability_validation_checks_targets_and_budgets_before_traversing() {
        let invalid = [
            word(0xfc, 0xff, 0xff, 0x7f, 1, 0, 0, 0), // distant struct
            word(1, 0, 0, 0, 0x12, 0, 0, 0),          // truncated data list
            word(1, 0, 0, 0, 0x0e, 0, 0, 0),          // truncated pointer list
            word(2, 0, 0, 0, 1, 0, 0, 0),             // missing far segment
            word(7, 0, 0, 0, 0, 0, 0, 0),             // unknown pointer tag
        ];
        for tag in invalid {
            let words = [tag];
            let segments = [Word::words_to_bytes(&words)];
            let reader = Reader::new(SegmentArray::new(&segments), ReaderOptions::new());
            assert!(reader
                .get_root::<any_pointer::Reader>()
                .unwrap()
                .validate_capability_indices(2)
                .is_err());
        }
        let words = [word(0, 0, 0, 0, 0, 0, 1, 0), word(3, 0, 0, 0, 1, 0, 0, 0)];
        for (count, options, succeeds) in [
            (1, ReaderOptions::new(), false),
            (2, ReaderOptions::new(), true),
            (
                2,
                ReaderOptions {
                    nesting_limit: 0,
                    ..ReaderOptions::new()
                },
                false,
            ),
            (
                2,
                ReaderOptions {
                    traversal_limit_in_words: Some(1),
                    ..ReaderOptions::new()
                },
                false,
            ),
        ] {
            let segments = [Word::words_to_bytes(&words)];
            let reader = Reader::new(SegmentArray::new(&segments), options);
            let result = reader
                .get_root::<any_pointer::Reader>()
                .and_then(|root| root.validate_capability_indices(count));
            assert_eq!(result.is_ok(), succeeds);
        }
        // A self-referential struct must stop at the reader's nesting limit.
        let words = [word(0xfc, 0xff, 0xff, 0xff, 0, 0, 1, 0)];
        let segments = [Word::words_to_bytes(&words)];
        let reader = Reader::new(
            SegmentArray::new(&segments),
            ReaderOptions {
                nesting_limit: 4,
                ..ReaderOptions::new()
            },
        );
        assert!(reader
            .get_root::<any_pointer::Reader>()
            .unwrap()
            .validate_capability_indices(0)
            .is_err());
    }
}
