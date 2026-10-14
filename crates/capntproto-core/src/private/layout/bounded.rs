//! Fallible layout transitions used by the restricted fixed-buffer editors.
#![deny(unsafe_op_in_unsafe_fn)]
use super::*;

impl PointerReader<'_> {
    pub(crate) fn validate_capability_indices(&self, count: u32) -> Result<()> {
        let mut current = *self;
        loop {
            if current.is_null() {
                return Ok(());
            }
            // SAFETY: a non-default PointerReader owns a validated pointer word;
            // is_null above also excludes the default reader's null address. Reading
            // its tag does not follow its target. get_struct/get_list below perform
            // the complete target, traversal-budget and nesting checks exactly once.
            let kind = match unsafe { (*current.pointer).kind() } {
                WirePointerKind::Struct => {
                    if current.nesting_limit <= 0 {
                        // The general validator classifies (and bounds the target
                        // offset) before reporting an exhausted nesting budget.
                        current.get_pointer_type()?;
                        return Err(Error::from_kind(
                            ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles,
                        ));
                    }
                    // SAFETY: self owns a validated non-null pointer word. Validate
                    // the complete struct before deriving or reading its pointer
                    // section. Every recursive reader stays within that section,
                    // keeps the original arena/cap table, and decrements nesting.
                    // Data-only structs require no temporary StructReader.
                    unsafe {
                        let data = usize::from((*current.pointer).struct_data_size());
                        let pointers = usize::from((*current.pointer).struct_ptr_count());
                        let body = current.arena.check_offset_and_read(
                            current.segment_id,
                            current.pointer.cast(),
                            (*current.pointer).offset_in_words(),
                            data + pointers,
                        )?;
                        let section = body.add(data * BYTES_PER_WORD).cast::<WirePointer>();
                        if pointers == 1 {
                            current.pointer = section;
                            current.nesting_limit -= 1;
                            continue;
                        }
                        for index in 0..pointers {
                            if (*section.add(index)).is_null() {
                                continue;
                            }
                            PointerReader {
                                pointer: section.add(index),
                                nesting_limit: current.nesting_limit - 1,
                                ..current
                            }
                            .validate_capability_indices(count)?;
                        }
                    }
                    return Ok(());
                }
                WirePointerKind::List => {
                    // A near primitive list has no descendants or capability slots.
                    // Validate its full extent and charge exactly the ordinary list
                    // traversal, without constructing a general ListReader.
                    // SAFETY: is_null above excludes a default reader. This is a
                    // validated pointer word, not its (still untrusted) target.
                    let size = unsafe { (*current.pointer).list_element_size() };
                    if !matches!(size, ElementSize::Pointer | ElementSize::InlineComposite) {
                        if current.nesting_limit <= 0 {
                            current.get_pointer_type()?;
                            return Err(Error::from_kind(ErrorKind::NestingLimitExceeded));
                        }
                        // SAFETY: only the validated pointer word is inspected. The
                        // arena checks the entire target before forming any pointer;
                        // no target is dereferenced by this primitive-only path.
                        let elements = unsafe {
                            let elements = (*current.pointer).list_element_count();
                            let words = wire_helpers::round_bits_up_to_words(
                                u64::from(elements) * u64::from(data_bits_per_element(size)),
                            );
                            current.arena.check_offset_and_read(
                                current.segment_id,
                                current.pointer.cast(),
                                (*current.pointer).offset_in_words(),
                                words as usize,
                            )?;
                            elements
                        };
                        if size == ElementSize::Void {
                            current.arena.amplified_read(u64::from(elements))?;
                        }
                        return Ok(());
                    }
                    PointerType::List
                }
                // Landing pads and unknown/capability tags retain the full checked
                // decoding path. No far target is read before validating its pad.
                _ => current.get_pointer_type()?,
            };
            match kind {
                PointerType::Null => (),
                PointerType::Capability => {
                    if current.get_capability_index()? >= count {
                        return Err(Error::from_kind(
                            ErrorKind::MessageContainsInvalidCapabilityPointer,
                        ));
                    }
                }
                PointerType::Struct => {
                    let value = current.get_struct(None)?;
                    for i in 0..usize::from(value.get_pointer_section_size()) {
                        value
                            .get_pointer_field(i)
                            .validate_capability_indices(count)?;
                    }
                }
                PointerType::List => {
                    let value = current.get_list_any_size(ptr::null())?;
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
            return Ok(());
        }
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
    use super::*;
    use crate::{
        any_pointer,
        message::{Reader, ReaderOptions, SegmentArray},
        word, Word,
    };

    // Use the general readers as an independent oracle for the direct near
    // traversal. Compare both errors and the remaining traversal budget.
    fn general(root: PointerReader<'_>, count: u32) -> Result<()> {
        match root.get_pointer_type()? {
            PointerType::Null => (),
            PointerType::Capability => {
                if root.get_capability_index()? >= count {
                    return Err(Error::from_kind(
                        ErrorKind::MessageContainsInvalidCapabilityPointer,
                    ));
                }
            }
            PointerType::Struct => {
                let value = root.get_struct(None)?;
                for index in 0..usize::from(value.get_pointer_section_size()) {
                    general(value.get_pointer_field(index), count)?;
                }
            }
            PointerType::List => {
                let value = root.get_list_any_size(ptr::null())?;
                match value.element_size {
                    ElementSize::Pointer => {
                        for index in 0..value.len() {
                            general(value.get_pointer_element(index), count)?;
                        }
                    }
                    ElementSize::InlineComposite => {
                        for index in 0..value.len() {
                            let element = value.get_struct_element(index);
                            for field in 0..usize::from(element.get_pointer_section_size()) {
                                general(element.get_pointer_field(field), count)?;
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        Ok(())
    }
    fn encoded(low: u32, high: u32) -> Word {
        let a = low.to_le_bytes();
        let b = high.to_le_bytes();
        word(a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3])
    }
    fn compare(words: &[Word], count: u32) {
        let segments = [Word::words_to_bytes(words)];
        for (limit, nesting) in [(None, 0), (Some(1), 8), (Some(8), 8), (None, 8)] {
            let options = ReaderOptions {
                traversal_limit_in_words: limit,
                nesting_limit: nesting,
            };
            let run = |direct| {
                let reader = Reader::new(SegmentArray::new(&segments), options);
                let walk = || {
                    reader
                        .get_root::<any_pointer::Reader>()
                        .and_then(|root| {
                            if direct {
                                root.validate_capability_indices(count)
                            } else {
                                general(root.reader, count)
                            }
                        })
                        .map_err(|error| error.kind)
                };
                (walk(), walk())
            };
            assert_eq!(
                run(true),
                run(false),
                "limit={limit:?}, nesting={nesting}, count={count}"
            );
        }
    }
    #[test]
    fn direct_near_validation_matches_general_readers_and_budget_charges() {
        for size in 0..=5 {
            for count in [0, 1, 65] {
                for offset in [0, 4, 0xffff_fffc, 0x7fff_fffc] {
                    let mut words = [encoded(0, 0); 4];
                    words[0] = encoded(offset | 1, (count << 3) | size);
                    compare(&words, 0);
                }
            }
        }
        for data in 0..3 {
            for pointers in 0..3 {
                for offset in [0, 4, 0xffff_fffc, 0x7fff_fffc] {
                    let mut words = [encoded(0, 0); 4];
                    words[0] = encoded(offset, data | (pointers << 16));
                    words[1] = encoded(3, 1);
                    for count in [0, 2] {
                        compare(&words, count);
                    }
                }
            }
        }
        compare(&[encoded(0xffff_fffc, 1 << 16)], 0); // recursive struct
    }

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
