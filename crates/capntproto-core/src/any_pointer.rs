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

//! Untyped pointer that can be cast to any struct, list, or capability type.

#[cfg(feature = "alloc")]
use crate::capability::FromClientHook;
#[cfg(feature = "alloc")]
use crate::private::capability::{ClientHook, PipelineHook, PipelineOp};
use crate::private::layout::{PointerBuilder, PointerReader};
use crate::traits::{FromPointerBuilder, FromPointerReader, SetterInput};
use crate::Result;

pub use crate::private::layout::PointerType;

#[derive(Copy, Clone)]
pub struct Owned(());

impl crate::traits::Owned for Owned {
    type Reader<'a> = Reader<'a>;
    type Builder<'a> = Builder<'a>;
}

impl crate::introspect::Introspect for Owned {
    fn introspect() -> crate::introspect::Type {
        crate::introspect::TypeVariant::AnyPointer.into()
    }
}

impl crate::traits::Pipelined for Owned {
    type Pipeline = Pipeline;
}

#[derive(Copy, Clone)]
pub struct Reader<'a> {
    pub(crate) reader: PointerReader<'a>,
}

impl<'a> Reader<'a> {
    pub fn new(reader: PointerReader<'_>) -> Reader<'_> {
        Reader { reader }
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        self.reader.is_null()
    }

    /// Classifies the pointer, following far pointers and validating their tags.
    /// This does not validate the entire target or extract a capability.
    pub fn get_pointer_type(&self) -> Result<PointerType> {
        self.reader.get_pointer_type()
    }

    /// Gets the total size of the target and all of its children. Does not count far pointer overhead.
    pub fn target_size(&self) -> Result<crate::MessageSize> {
        self.reader.total_size()
    }

    /// Compares the complete pointed-to values without consulting a schema.
    ///
    /// Structs ignore trailing zero data and null pointers. Lists must have the
    /// same length and physical element encoding; unused bit-list padding is
    /// ignored. Capability pairs yield [`crate::Equality::UnknownContainsCapabilities`],
    /// even for the same client. No client hook is inspected or invoked.
    ///
    /// Reading errors are propagated, including traversal and nesting limits.
    /// Comparison stops at the first definite difference; it does not validate
    /// the rest of an unequal message. Use bounded message readers for untrusted
    /// data, as with other recursive message operations.
    ///
    /// ```
    /// use capnp::{any_pointer, message, Equality};
    /// let mut message = message::Builder::new_default();
    /// let mut root = message.init_root::<any_pointer::Builder>();
    /// root.set_as::<capnp::text::Owned>("hello")?;
    /// assert_eq!(root.equals(root.as_reader())?, Equality::Equal);
    /// # Ok::<(), capnp::Error>(())
    /// ```
    pub fn equals(&self, right: Reader<'_>) -> Result<crate::Equality> {
        self.reader.equals(right.reader)
    }

    #[inline]
    pub fn get_as<T: FromPointerReader<'a>>(&self) -> Result<T> {
        FromPointerReader::get_from_pointer(&self.reader, None)
    }

    #[cfg(feature = "alloc")]
    pub fn get_as_capability<T: FromClientHook>(&self) -> Result<T> {
        Ok(FromClientHook::new(self.reader.get_capability()?))
    }

    //# Used by RPC system to implement pipelining. Applications
    //# generally shouldn't use this directly.
    #[cfg(feature = "alloc")]
    pub fn get_pipelined_cap(
        &self,
        ops: &[PipelineOp],
    ) -> Result<alloc::boxed::Box<dyn ClientHook>> {
        let mut pointer = self.reader;

        for op in ops {
            match *op {
                PipelineOp::Noop => {}
                PipelineOp::GetPointerField(idx) => {
                    pointer = pointer.get_struct(None)?.get_pointer_field(idx as usize);
                }
            }
        }

        pointer.get_capability()
    }
}

impl<'a> FromPointerReader<'a> for Reader<'a> {
    fn get_from_pointer(
        reader: &PointerReader<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Reader<'a>> {
        if default.is_some() {
            panic!("Unsupported: any_pointer with a default value.");
        }
        Ok(Reader { reader: *reader })
    }
}

impl<'a> crate::traits::SetterInput<Owned> for Reader<'a> {
    #[inline]
    fn set_pointer_builder<'b>(
        mut pointer: crate::private::layout::PointerBuilder<'b>,
        value: Reader<'a>,
        canonicalize: bool,
    ) -> Result<()> {
        pointer.copy_from(value.reader, canonicalize)
    }
}

#[cfg(feature = "alloc")]
impl<'a> crate::traits::Imbue<'a> for Reader<'a> {
    fn imbue(&mut self, cap_table: &'a crate::private::layout::CapTable) {
        self.reader
            .imbue(crate::private::layout::CapTableReader::Plain(cap_table));
    }
}

pub struct Builder<'a> {
    pub(crate) builder: PointerBuilder<'a>,
}

impl<'a> Builder<'a> {
    pub fn new(builder: PointerBuilder<'a>) -> Builder<'a> {
        Builder { builder }
    }

    pub fn reborrow(&mut self) -> Builder<'_> {
        Builder {
            builder: self.builder.reborrow(),
        }
    }

    pub fn is_null(&self) -> bool {
        self.builder.is_null()
    }

    pub fn get_pointer_type(&self) -> Result<PointerType> {
        self.as_reader().get_pointer_type()
    }

    /// Replaces this pointer with a zero-initialized struct of the given layout.
    /// Sizes are in words (eight bytes) and pointer slots, respectively.
    pub fn init_as_any_struct(
        self,
        data_word_count: u16,
        pointer_count: u16,
    ) -> crate::any_struct::Builder<'a> {
        crate::any_struct::Builder::new(self.builder.init_struct(
            crate::private::layout::StructSize {
                data: data_word_count,
                pointers: pointer_count,
            },
        ))
    }

    /// Replaces the pointer with a zero-initialized non-struct list. Invalid
    /// encodings/counts return an error before changing the existing value.
    pub fn init_as_any_list(
        self,
        element_size: crate::any_list::ElementSize,
        count: u32,
    ) -> Result<crate::any_list::Builder<'a>> {
        if element_size == crate::any_list::ElementSize::InlineComposite {
            return Err(crate::Error::from_kind(crate::ErrorKind::TypeMismatch));
        }
        if count >= 1 << 29 {
            return Err(crate::Error::from_kind(crate::ErrorKind::MessageTooLarge(
                count as usize,
            )));
        }
        Ok(crate::any_list::Builder::new(
            self.builder.init_list(element_size, count),
        ))
    }

    /// Allocates an inline struct list with explicit per-element word/pointer
    /// counts. Rejects wire count/size overflow before replacing the old value.
    pub fn init_as_list_of_any_struct(
        self,
        data_word_count: u16,
        pointer_count: u16,
        count: u32,
    ) -> Result<crate::any_struct_list::Builder<'a>> {
        let size = crate::private::layout::StructSize {
            data: data_word_count,
            pointers: pointer_count,
        };
        if count >= 1 << 30 || u64::from(count) * u64::from(size.total()) >= 1 << 29 {
            return Err(crate::Error::from_kind(crate::ErrorKind::MessageTooLarge(
                count as usize,
            )));
        }
        Ok(crate::any_struct_list::Builder::new(
            self.builder.init_struct_list(count, size),
        ))
    }

    /// Gets the total size of the target and all of its children. Does not count far pointer overhead.
    pub fn target_size(&self) -> Result<crate::MessageSize> {
        self.builder.as_reader().total_size()
    }

    /// Borrows a read-only view without consuming this builder.
    pub fn as_reader(&self) -> Reader<'_> {
        Reader::new(self.builder.as_reader())
    }

    /// Compares this value using the same rules as [`Reader::equals`].
    pub fn equals(&self, right: Reader<'_>) -> Result<crate::Equality> {
        self.as_reader().equals(right)
    }

    pub fn get_as<T: FromPointerBuilder<'a>>(self) -> Result<T> {
        FromPointerBuilder::get_from_pointer(self.builder, None)
    }

    pub fn init_as<T: FromPointerBuilder<'a>>(self) -> T {
        FromPointerBuilder::init_pointer(self.builder, 0)
    }

    pub fn initn_as<T: FromPointerBuilder<'a>>(self, size: u32) -> T {
        FromPointerBuilder::init_pointer(self.builder, size)
    }

    pub fn set_as<T: crate::traits::Owned>(&mut self, value: impl SetterInput<T>) -> Result<()> {
        SetterInput::set_pointer_builder(self.builder.reborrow(), value, false)
    }

    // XXX value should be a user client.
    #[cfg(feature = "alloc")]
    pub fn set_as_capability(&mut self, value: alloc::boxed::Box<dyn ClientHook>) {
        self.builder.set_capability(value);
    }

    #[inline]
    pub fn clear(&mut self) {
        self.builder.clear()
    }

    pub fn into_reader(self) -> Reader<'a> {
        Reader {
            reader: self.builder.into_reader(),
        }
    }
}

impl<'a> FromPointerBuilder<'a> for Builder<'a> {
    fn init_pointer(mut builder: PointerBuilder<'a>, _len: u32) -> Builder<'a> {
        if !builder.is_null() {
            builder.clear();
        }
        Builder { builder }
    }
    fn get_from_pointer(
        builder: PointerBuilder<'a>,
        default: Option<&'a [crate::Word]>,
    ) -> Result<Builder<'a>> {
        if default.is_some() {
            panic!("AnyPointer defaults are unsupported")
        }
        Ok(Builder { builder })
    }
}

#[cfg(feature = "alloc")]
impl<'a> crate::traits::ImbueMut<'a> for Builder<'a> {
    fn imbue_mut(&mut self, cap_table: &'a mut crate::private::layout::CapTable) {
        self.builder
            .imbue(crate::private::layout::CapTableBuilder::Plain(cap_table));
    }
}

pub struct Pipeline {
    // XXX this should not be public
    #[cfg(feature = "alloc")]
    pub hook: alloc::boxed::Box<dyn PipelineHook>,

    #[cfg(feature = "alloc")]
    ops: alloc::vec::Vec<PipelineOp>,
}

impl Pipeline {
    #[cfg(feature = "alloc")]
    pub fn new(hook: alloc::boxed::Box<dyn PipelineHook>) -> Self {
        Self {
            hook,
            ops: alloc::vec::Vec::new(),
        }
    }

    #[cfg(feature = "alloc")]
    pub fn noop(&self) -> Self {
        Self {
            hook: self.hook.add_ref(),
            ops: self.ops.clone(),
        }
    }

    #[cfg(not(feature = "alloc"))]
    pub fn noop(&self) -> Self {
        Self {}
    }

    #[cfg(feature = "alloc")]
    pub fn get_pointer_field(&self, pointer_index: u16) -> Self {
        let mut new_ops = alloc::vec::Vec::with_capacity(self.ops.len() + 1);
        for op in &self.ops {
            new_ops.push(*op)
        }
        new_ops.push(PipelineOp::GetPointerField(pointer_index));
        Self {
            hook: self.hook.add_ref(),
            ops: new_ops,
        }
    }

    #[cfg(not(feature = "alloc"))]
    pub fn get_pointer_field(&self, _pointer_index: u16) -> Self {
        Self {}
    }

    #[cfg(feature = "alloc")]
    pub fn as_cap(&self) -> alloc::boxed::Box<dyn ClientHook> {
        self.hook.get_pipelined_cap(&self.ops)
    }

    /// Consume this pipeline as a hook rooted at its current pointer path.
    #[cfg(feature = "alloc")]
    pub fn into_hook(self) -> alloc::boxed::Box<dyn PipelineHook> {
        if self.ops.is_empty() {
            self.hook
        } else {
            alloc::boxed::Box::new(self)
        }
    }
}

#[cfg(feature = "alloc")]
impl PipelineHook for Pipeline {
    fn add_ref(&self) -> alloc::boxed::Box<dyn PipelineHook> {
        alloc::boxed::Box::new(self.noop())
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> alloc::boxed::Box<dyn ClientHook> {
        let mut path = self.ops.clone();
        path.extend_from_slice(ops);
        self.hook.get_pipelined_cap_move(path)
    }
}

impl crate::capability::FromTypelessPipeline for Pipeline {
    fn new(typeless: Pipeline) -> Self {
        typeless
    }
}

impl crate::capability::IntoTypelessPipeline for Pipeline {
    fn into_typeless_pipeline(self) -> Pipeline {
        self
    }
}

impl<'a> From<Reader<'a>> for crate::dynamic_value::Reader<'a> {
    fn from(a: Reader<'a>) -> crate::dynamic_value::Reader<'a> {
        crate::dynamic_value::Reader::AnyPointer(a)
    }
}

impl<'a> From<Builder<'a>> for crate::dynamic_value::Builder<'a> {
    fn from(a: Builder<'a>) -> crate::dynamic_value::Builder<'a> {
        crate::dynamic_value::Builder::AnyPointer(a)
    }
}

#[cfg(feature = "alloc")]
#[test]
fn init_clears_value() {
    let mut message = crate::message::Builder::new_default();
    {
        let root: crate::any_pointer::Builder = message.init_root();
        let mut list: crate::primitive_list::Builder<u16> = root.initn_as(10);
        for idx in 0..10 {
            list.set(idx, idx as u16);
        }
    }

    {
        let root: crate::any_pointer::Builder = message.init_root();
        assert!(root.is_null());
    }

    let mut output: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    crate::serialize::write_message(&mut output, &message).unwrap();
    assert_eq!(output.len(), 40);
    for byte in &output[8..] {
        // Everything not in the message header is zero.
        assert_eq!(*byte, 0u8);
    }
}
