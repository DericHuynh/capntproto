// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! Copy concatenation and tail allocation resizing for uniquely owned lists.
use super::*;

impl PointerBuilder<'_> {
    #[cfg(feature = "alloc")]
    pub(super) fn grow_owned_list_in_place(
        &mut self,
        old_count: u32,
        count: u32,
        element: ElementSize,
        step: u32,
    ) -> Result<bool> {
        let old_words =
            wire_helpers::round_bits_up_to_words(u64::from(old_count) * u64::from(step));
        let new_words = wire_helpers::round_bits_up_to_words(u64::from(count) * u64::from(step));
        let tag_words = u32::from(element == InlineComposite);
        // SAFETY: the caller validated the physical shape and wire count. The
        // exclusive owner has no outstanding views. The arena only extends a
        // tail allocation into unused, zero-initialized storage.
        unsafe {
            let (body, tag, segment) = wire_helpers::follow_builder_fars(
                self.arena,
                self.pointer,
                WirePointer::mut_target(self.pointer),
                self.segment_id,
            )?;
            let (start, _) = self.arena.get_segment(segment)?;
            let offset = u32::try_from((body as usize - start as usize) / 8).unwrap();
            if old_words != new_words
                && !self.arena.try_resize_allocation(
                    segment,
                    offset,
                    old_words + tag_words,
                    new_words + tag_words,
                )
            {
                return Ok(false);
            }
            if element == InlineComposite {
                (*tag).set_list_inline_composite(new_words);
                (*(body as *mut WirePointer)).set_kind_and_inline_composite_list_element_count(
                    WirePointerKind::Struct,
                    count,
                );
            } else {
                (*tag).set_list_size_and_count(element, count);
            }
        }
        Ok(true)
    }

    pub(crate) fn init_concat_list(
        self,
        mut element: ElementSize,
        mut size: StructSize,
        lists: &[ListReader<'_>],
    ) -> Result<()> {
        let mut count = 0u32;
        for list in lists {
            count = count
                .checked_add(list.len())
                .filter(|n| *n < (1 << 29))
                .ok_or_else(|| Error::failed("concatenated list exceeds wire limit".into()))?;
            // A null pointer has a default Void encoding, regardless of the
            // declared element type. It contributes no physical layout.
            if list.ptr.is_null() {
                continue;
            }
            if element != list.element_size {
                if element == Bit || list.element_size == Bit {
                    return Err(Error::from_kind(
                        ErrorKind::FoundBitListWhereStructListWasExpected,
                    ));
                }
                element = InlineComposite;
            }
            // Struct sizes come from a validated 16-bit word count, or a
            // primitive encoding with at most one word of data.
            size.data = size.data.max(
                u16::try_from(list.struct_data_size.div_ceil(64))
                    .expect("validated struct data size fits the wire header"),
            );
            size.pointers = size.pointers.max(list.struct_pointer_count);
        }
        if element == InlineComposite && u64::from(count) * u64::from(size.total()) >= (1 << 29) {
            return Err(Error::failed(
                "concatenated struct list exceeds wire limit".into(),
            ));
        }
        let mut target = if element == InlineComposite {
            self.init_struct_list(count, size)
        } else {
            self.init_list(element, count)
        };
        let mut pos = 0;
        for list in lists {
            match element {
                InlineComposite => {
                    for i in 0..list.len() {
                        target
                            .reborrow()
                            .get_struct_element(pos + i)
                            .copy_content_from(&list.get_struct_element(i))?;
                    }
                }
                Pointer => {
                    for i in 0..list.len() {
                        target
                            .reborrow()
                            .get_pointer_element(pos + i)
                            .copy_from(list.get_pointer_element(i), false)?;
                    }
                }
                Bit => {
                    for i in 0..list.len() {
                        bool::set(&target, pos + i, bool::get(list, i));
                    }
                }
                Void => (),
                _ => {
                    if !list.is_empty() {
                        let width = target.step as usize / 8;
                        // SAFETY: all nonempty primitive inputs have the same encoding,
                        // otherwise the planning pass selected InlineComposite. The
                        // planning pass checked total count; pos tracks copied entries.
                        // Fresh destination storage is disjoint from every source.
                        unsafe {
                            ptr::copy_nonoverlapping(
                                list.ptr,
                                target.ptr.add(pos as usize * width),
                                list.len() as usize * width,
                            );
                        }
                    }
                }
            }
            pos += list.len();
        }
        Ok(())
    }

    pub(super) fn shrink_owned_list(&mut self, count: u32, text: bool) -> Result<()> {
        let (old_count, element, step, data_bits, pointers) = {
            let list = self.as_reader().get_list_any_size(ptr::null())?;
            (
                list.element_count,
                list.element_size,
                list.step,
                list.struct_data_size,
                list.struct_pointer_count,
            )
        };
        debug_assert!(count < old_count);
        debug_assert!(!text || count > 0);
        // SAFETY: the reader checked the entire physical list, including its
        // landing pad. This exclusively borrowed builder owns mutable storage.
        // Only truncated elements and padding are cleared; retained bytes stay
        // at their original addresses. No allocation is performed.
        unsafe {
            let (body, tag, segment) = wire_helpers::follow_builder_fars(
                self.arena,
                self.pointer,
                WirePointer::mut_target(self.pointer),
                self.segment_id,
            )?;
            let data = if element == InlineComposite {
                body.add(8)
            } else {
                body
            };
            if pointers != 0 {
                for i in count..old_count {
                    let fields = data.add(i as usize * (step as usize / 8) + data_bits as usize / 8)
                        as *mut WirePointer;
                    for p in 0..pointers as usize {
                        PointerBuilder {
                            arena: self.arena,
                            segment_id: segment,
                            cap_table: self.cap_table,
                            pointer: fields.add(p),
                        }
                        .clear();
                    }
                }
            }
            let kept_bits = u64::from(if text { count - 1 } else { count }) * u64::from(step);
            let old_bytes =
                wire_helpers::round_bits_up_to_words(u64::from(old_count) * u64::from(step))
                    as usize
                    * 8;
            let first_byte = (kept_bits / 8) as usize;
            let partial = (kept_bits % 8) as u8;
            if partial != 0 {
                *data.add(first_byte) &= (1 << partial) - 1;
            }
            let clear_from = first_byte + usize::from(partial != 0);
            if clear_from < old_bytes {
                ptr::write_bytes(data.add(clear_from), 0, old_bytes - clear_from);
            }
            if element == InlineComposite {
                (*tag).set_list_inline_composite(count * (step / 64));
                (*(body as *mut WirePointer)).set_kind_and_inline_composite_list_element_count(
                    WirePointerKind::Struct,
                    count,
                );
            } else {
                (*tag).set_list_size_and_count(element, count);
            }
            let (start, _) = self.arena.get_segment(segment)?;
            let offset = u32::try_from((body as usize - start as usize) / 8).unwrap();
            let tag_words = u32::from(element == InlineComposite);
            let old_words = u32::try_from(old_bytes / 8).unwrap() + tag_words;
            let new_words =
                wire_helpers::round_bits_up_to_words(u64::from(count) * u64::from(step))
                    + tag_words;
            self.arena
                .try_resize_allocation(segment, offset, old_words, new_words);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_preserves_allocation_and_clears_wire_tail_and_padding() -> Result<()> {
        for small in [false, true] {
            for (element, text) in [(Bit, false), (Byte, false), (Byte, true)] {
                let allocator = crate::message::HeapAllocator::new();
                let allocator = if small {
                    allocator
                        .first_segment_words(1)
                        .allocation_strategy(crate::message::AllocationStrategy::FixedSize)
                } else {
                    allocator
                };
                let mut message = crate::message::Builder::new(allocator);
                let mut root = message.init_root::<crate::any_pointer::Builder>();
                if text {
                    root.builder
                        .reborrow()
                        .init_text(17)
                        .as_bytes_mut()
                        .fill(255);
                } else {
                    root.builder
                        .reborrow()
                        .init_list(element, 17)
                        .as_raw_bytes()
                        .fill(255);
                }
                let before = root.builder.as_reader().get_list_any_size(ptr::null())?.ptr;
                let sizes: alloc::vec::Vec<_> = message
                    .get_segments_for_output()
                    .iter()
                    .map(|s| s.len())
                    .collect();
                message
                    .get_root::<crate::any_pointer::Builder>()?
                    .builder
                    .resize_owned_list(if text { 4 } else { 3 }, text)?;
                let after = message
                    .get_root_as_reader::<crate::any_pointer::Reader>()?
                    .reader
                    .get_list_any_size(ptr::null())?
                    .ptr;
                assert_eq!(before, after);
                let segments = message.get_segments_for_output();
                assert!(segments.iter().map(|s| s.len()).sum::<usize>() <= sizes.iter().sum());
                let old_bytes = if element == Bit { 8 } else { 24 };
                // SAFETY: the allocator still owns these bytes, even when the logical
                // segment tail was reclaimed. Inspect clearing before reuse.
                let body = unsafe { core::slice::from_raw_parts(after, old_bytes) };
                if element == Bit {
                    assert_eq!(body[0], 7);
                    assert!(body[1..].iter().all(|b| *b == 0));
                } else {
                    assert_eq!(&body[..3], &[255; 3]);
                    assert!(body[3..].iter().all(|b| *b == 0));
                }
            }
        }
        Ok(())
    }
}
