//! Relocate a local Call template without rebuilding its graph.
//! Relative pointers remain valid when the entire frame is copied. Only the
//! two scalar IDs and advisory bits change. The private publication path uses
//! the original validation certificate; changed reader limits revalidate it.
use super::*;
use capnp::traits::IntoInternalStructReader;
use capntproto_rpc_engine::ids::QuestionId;
mod capabilities;
pub(super) use capabilities::EncodedCapabilities;

/// Reusable immutable Call header. Each request receives independent storage;
/// method IDs are set through the generated builder before selecting parameters.
#[cfg(feature = "alloc")]
pub struct PreparedCallTemplate {
    message: capnp::message::SelectedBuilderTemplate<capnp::message::HeapAllocator>,
    offsets: Option<(usize, usize)>,
    method: (u64, u16),
}
#[cfg(feature = "alloc")]
impl Default for PreparedCallTemplate {
    fn default() -> Self {
        Self::new(0, 0)
    }
}
#[cfg(feature = "alloc")]
impl PreparedCallTemplate {
    pub fn new(interface: u64, method: u16) -> Self {
        let prepared = PreparedCall::new(
            capnp::message::Builder::new(
                capnp::message::HeapAllocator::new().first_segment_words(32),
            ),
            interface,
            method,
        )
        .expect("empty Call envelope");
        let offsets = prepared.offsets;
        Self {
            message: prepared
                .message
                .into_template()
                .expect("single-segment data-only Call envelope"),
            offsets,
            method: (interface, method),
        }
    }
}

/// An owning Call envelope whose only mutable public field is its parameters.
/// The immutable header is constructed here, so publication need not rediscover
/// its method, target layout, empty descriptor table or scheduling flags. The
/// Checked writers validate the application payload before publication. Native
/// writers leave payload validation to incoming reads, as legacy/C++ do.
#[cfg(feature = "alloc")]
pub struct PreparedCall<A: capnp::message::Allocator> {
    message: capnp::message::SelectedBuilder<A>,
    method: calls::DynamicCall,
    offsets: Option<(usize, usize)>,
    owner: Option<alloc::rc::Weak<PreparedCall<capnp::message::HeapAllocator>>>,
    validation: core::cell::Cell<Option<Validation>>,
}
#[cfg(feature = "alloc")]
impl<A: capnp::message::Allocator> PreparedCall<A> {
    #[inline(always)]
    pub fn from_template(
        template: &PreparedCallTemplate,
        allocator: A,
        interface: u64,
        method: u16,
    ) -> capnp::Result<Self> {
        let message = template.message.instantiate(allocator);
        let method = calls::DynamicCall {
            interface,
            method,
            no_promise_pipelining: true,
            only_promise_pipeline: false,
        };
        if template.method == (method.interface, method.method) {
            return Ok(Self {
                message,
                offsets: template.offsets,
                method,
                owner: None,
                validation: Default::default(),
            });
        }
        // Keep the common immutable-header copy separate from reselecting an
        // envelope for another method, avoiding a large builder join temporary.
        let message = message.into_inner().select(|root| {
            let root = root.get_as::<rpc_capnp::message::Builder>()?;
            let rpc_capnp::message::Call(call) = root.which().expect("private Call discriminant")
            else {
                unreachable!("private Call envelope")
            };
            let mut call = call?;
            call.set_interface_id(method.interface);
            call.set_method_id(method.method);
            Ok(call.get_params()?.get_content())
        })?;
        Ok(Self {
            message,
            offsets: template.offsets,
            method,
            owner: None,
            validation: Default::default(),
        })
    }
    pub fn new(
        message: capnp::message::Builder<A>,
        interface: u64,
        method: u16,
    ) -> capnp::Result<Self> {
        let mut addresses = (0, 0);
        let message = message.select(|root| {
            let mut call = root.init_as::<rpc_capnp::message::Builder>().init_call();
            call.set_interface_id(interface);
            call.set_method_id(method);
            call.set_no_promise_pipelining(true);
            addresses.0 = call
                .reborrow_as_reader()
                .into_internal_struct_reader()
                .get_data_section_as_blob()
                .as_ptr() as usize;
            let mut target = call.reborrow().init_target();
            target.set_imported_cap(0);
            addresses.1 = target
                .reborrow_as_reader()
                .into_internal_struct_reader()
                .get_data_section_as_blob()
                .as_ptr() as usize;
            Ok(call.init_params().get_content())
        })?;
        let offsets = match message.message().get_segments_for_output() {
            capnp::OutputSegments::SingleSegment([segment]) => {
                let start = segment.as_ptr() as usize;
                Some((addresses.0 - start, addresses.1 - start))
            }
            _ => None,
        };
        Ok(Self {
            message,
            offsets,
            owner: None,
            validation: Default::default(),
            method: calls::DynamicCall {
                interface,
                method,
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            },
        })
    }
    pub fn params(&mut self) -> capnp::any_pointer::Builder<'_> {
        self.validation.set(None);
        self.message.get()
    }
    /// Read the same selected parameter field used by the mutable builder.
    pub fn params_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.message.get_reader()
    }
    pub fn message(&self) -> &capnp::message::Builder<A> {
        self.message.message()
    }
    pub fn into_inner(self) -> capnp::message::Builder<A> {
        self.message.into_inner()
    }
    /// Return a data-only certificate for one or two segments, or let the general
    /// codec handle more segments. Any capability pointer in the content fails
    /// validation; callers with a capability table use the prepared-table writer.
    /// The caller may select the ordinary no-pipelining hint at publication;
    /// the copied private header receives that hint without editing the source.
    #[inline(always)]
    pub fn encoded(
        &self,
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<EncodedCall<'_>>, super::super::super::Error> {
        self.encode_layout(method, Some(options))
    }
    /// Retain a local Call layout without pre-reading its application payload.
    /// A Native writer can publish it directly; Checked writers still validate
    /// it before publication. Capability tables are supplied separately, so
    /// hosted callers use this only when their parameter hook table is empty.
    pub fn encoded_for_sending(
        &self,
        method: calls::DynamicCall,
    ) -> Result<Option<EncodedCall<'_>>, super::super::super::Error> {
        self.encode_layout(method, None)
    }
    pub(super) fn capability_layout(
        &self,
        method: calls::DynamicCall,
    ) -> Result<Option<EncodedCapabilities<'_>>, super::super::super::Error> {
        let Some(call) = self.encoded_for_sending(method)? else {
            return Ok(None);
        };
        // Only this private builder can supply the immutable envelope. Locate
        // its descriptor pointer once without visiting application content.
        let root = self
            .message()
            .get_root_as_reader::<rpc_capnp::message::Reader>()?;
        let rpc_capnp::message::Call(header) = root
            .which()
            .map_err(|_| super::super::super::Error::WrongMessage)?
        else {
            return Err(super::super::super::Error::WrongMessage);
        };
        let payload = header?.get_params()?.into_internal_struct_reader();
        let pointers = payload.get_pointer_section_as_blob();
        let table = (pointers.as_ptr() as usize)
            .checked_add(8)
            .and_then(|offset| offset.checked_sub(call.segment.as_ptr() as usize));
        let Some(table) = table.filter(|offset| {
            pointers.len() >= 16
                && offset
                    .checked_add(8)
                    .is_some_and(|end| end <= call.segment.len())
        }) else {
            return Ok(None);
        };
        Ok(Some(EncodedCapabilities { call, table }))
    }
    #[inline(always)]
    pub(super) fn check_method(
        &self,
        method: calls::DynamicCall,
    ) -> Result<(), super::super::super::Error> {
        if (self.method.interface, self.method.method) != (method.interface, method.method)
            || method.only_promise_pipeline
        {
            return Err(super::super::super::Error::WrongMessage);
        }
        Ok(())
    }
    #[inline(always)]
    fn encode_layout(
        &self,
        method: calls::DynamicCall,
        options: Option<capnp::message::ReaderOptions>,
    ) -> Result<Option<EncodedCall<'_>>, super::super::super::Error> {
        self.check_method(method)?;
        let Some((call, target)) = self.offsets else {
            return Ok(None);
        };
        use capnp::message::ReaderSegments;
        let message = self.message();
        let second = match message.len() {
            1 => None,
            2 => message.get_segment(1),
            _ => return Ok(None),
        };
        let segment = message.get_segment(0).expect("prepared Call root segment");
        if let Some(options) = options {
            let words = segment.len() / 8 + second.map_or(0, |segment| segment.len() / 8);
            if options
                .traversal_limit_in_words
                .is_some_and(|limit| words > limit)
            {
                return Err(
                    capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(words)).into(),
                );
            }
            if !self
                .validation
                .get()
                .is_some_and(|proof| proof.matches(0, options))
            {
                // The private immutable envelope has exactly these three struct
                // layouts. The public editor can only change the selected content.
                // Charge their normal traversal/nesting cost, then validate that
                // selection directly instead of decoding the same envelope again.
                // Its pointer-word charge replaces the ordinary message-root charge.
                let prefix_words = [
                    rpc_capnp::api::Message::SIZE,
                    rpc_capnp::api::Call::SIZE,
                    rpc_capnp::api::Payload::SIZE,
                ]
                .iter()
                .map(|size| usize::from(size.data) + usize::from(size.pointers))
                .sum::<usize>();
                let mut selected_options = options;
                selected_options.nesting_limit = options
                    .nesting_limit
                    .checked_sub(3)
                    .filter(|limit| *limit >= 0)
                    .ok_or_else(|| {
                        capnp::Error::from_kind(
                            capnp::ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles,
                        )
                    })?;
                if let Some(limit) = &mut selected_options.traversal_limit_in_words {
                    *limit = limit.checked_sub(prefix_words).ok_or_else(|| {
                        capnp::Error::from_kind(capnp::ErrorKind::ReadLimitExceeded)
                    })?;
                }
                self.message
                    .validate_capability_indices(0, selected_options)?;
                self.validation.set(Validation::new(0, options));
            }
        }
        Ok(Some(EncodedCall {
            segment,
            second,
            options,
            owner: self.owner.as_ref().and_then(alloc::rc::Weak::upgrade),
            template: PlainTemplate {
                only_promise_pipeline: false,
                method: (method.interface, method.method),
                no_promise_pipelining: method.no_promise_pipelining,
                call,
                target,
                target_data_words: core::num::NonZeroU16::new(1),
                length: segment.len(),
            },
        }))
    }
}

#[cfg(feature = "alloc")]
impl PreparedCall<capnp::message::HeapAllocator> {
    /// Freeze this private envelope so output may retain its payload segments.
    /// No mutable builder access is available through the returned owner.
    pub fn into_shared(mut self) -> alloc::rc::Rc<Self> {
        alloc::rc::Rc::new_cyclic(|owner| {
            self.owner = Some(owner.clone());
            self
        })
    }
}

#[cfg(feature = "alloc")]
struct FramedCallParts {
    header: [u8; 16],
    first: crate::owned::connection::retained::Envelope,
    source: alloc::rc::Rc<PreparedCall<capnp::message::HeapAllocator>>,
    capacity: usize,
}
#[cfg(feature = "alloc")]
impl messages::Parts for FramedCallParts {
    fn count(&self) -> usize {
        3
    }
    fn part(&self, index: usize) -> &[u8] {
        use capnp::message::ReaderSegments;
        match index {
            0 => &self.header,
            1 => self.first.bytes(),
            2 => self
                .source
                .message()
                .get_segment(1)
                .expect("frozen second segment"),
            _ => panic!("invalid part"),
        }
    }
    fn retained_capacity(&self) -> usize {
        self.capacity
    }
}

/// An immutable local Call constructed by a safe message builder.
/// Builder ownership excludes overlapping wire objects; changing scalar IDs
/// therefore cannot change parameters, pointers, or capability descriptors.
/// Raw wire bytes cannot construct this layout. Checked writers validate its
/// payload unless it already carries a matching validation certificate.
pub struct EncodedCall<'a> {
    segment: &'a [u8],
    second: Option<&'a [u8]>,
    template: PlainTemplate,
    options: Option<capnp::message::ReaderOptions>,
    #[cfg(feature = "alloc")]
    owner: Option<alloc::rc::Rc<PreparedCall<capnp::message::HeapAllocator>>>,
}

impl<'a> EncodedCall<'a> {
    #[cfg(feature = "alloc")]
    pub(super) fn retain_parts(
        &self,
        template: PlainTemplate,
        messages: &mut messages::Messages<'_, '_, '_>,
        question: QuestionId,
        target: ImportId,
        allow_handoff: bool,
    ) -> Result<Option<messages::SharedParts>, super::super::super::Error> {
        let (Some(source), Some(second)) = (&self.owner, self.second) else {
            return Ok(None);
        };
        // Copying a small message is cheaper than another shared owner. Keep
        // the retained path for a large second segment and a small envelope.
        if second.len() < 4096 || self.segment.len() > 4096 {
            return Ok(None);
        }
        let Some(source_capacity) = source.message().retained_segment_bytes() else {
            return Ok(None);
        };
        let mut first = crate::owned::connection::retained::Envelope::copy_from(self.segment);
        let body = first.bytes_mut();
        body[template.call..template.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
        body[template.target..template.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
        body[template.call + 16] = (body[template.call + 16] & !0b111)
            | u8::from(allow_handoff)
            | (u8::from(template.no_promise_pipelining) << 1);
        let capacity = first
            .capacity_bytes()
            .checked_add(16)
            .and_then(|n| n.checked_add(source_capacity))
            .ok_or(super::super::super::Error::Capacity)?;
        let mut header = [0; 16];
        header[..4].copy_from_slice(&1u32.to_le_bytes());
        header[4..8].copy_from_slice(
            &u32::try_from(self.segment.len() / 8)
                .map_err(|_| super::super::super::Error::Capacity)?
                .to_le_bytes(),
        );
        header[8..12].copy_from_slice(
            &u32::try_from(second.len() / 8)
                .map_err(|_| super::super::super::Error::Capacity)?
                .to_le_bytes(),
        );
        let source = FramedCallParts {
            header,
            first,
            source: source.clone(),
            capacity,
        };
        match messages.retain_parts(source) {
            Ok(parts) => Ok(parts),
            // Existing reusable contiguous storage may still fit. Its ordinary
            // copy path enforces the same budget and preserves retry behavior.
            Err(messages::Error::NoBuffer) => Ok(None),
            Err(error) => Err(super::super::super::Error::Storage(error)),
        }
    }
    /// Append a promised-answer transform without walking or relocating the
    /// parameter graph. Every original segment retains its index and relative
    /// offsets. The private builder certificate excludes overlapping objects.
    /// Larger transforms and older target layouts use the general encoder.
    #[allow(clippy::too_many_arguments)]
    fn append_promised<'pool>(
        &self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        parent: QuestionId,
        path: pipeline::Path<'_>,
        minimum: usize,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'pool>>, super::super::super::Error> {
        let Some(data_words) = self.template.target_data_words else {
            return Ok(None);
        };
        let pointer = self.template.target + usize::from(data_words.get()) * 8;
        // Bound stack use independently of the configured path capacity. This
        // buffer holds a root, a PromisedAnswer and sixteen transform entries.
        let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 20];
        if path.fields().len() > 16 {
            return Ok(None);
        }
        let mut target = capnp::message::FixedBuilder::new(&mut words)?;
        path.write(
            target.try_init_root::<rpc_capnp::api::PromisedAnswer>()?,
            parent,
        )?;
        let target = target.get_segments_for_output()[0];
        let appended = &target[8..];
        let Some(first_bytes) = self.segment.len().checked_add(appended.len()) else {
            return Ok(None);
        };
        let total_words = first_bytes / 8 + self.second.map_or(0, |second| second.len() / 8);
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| total_words > limit)
        {
            return Ok(None);
        }
        let Ok(first_words) = u32::try_from(first_bytes / 8) else {
            return Ok(None);
        };
        // Relocate the generated root pointer into MessageTarget's pointer
        // field. Its target starts immediately after the unchanged segment.
        // Only the signed 30-bit offset changes; retain the generated sizes.
        let offset = (self.segment.len() - pointer - 8) / 8;
        if offset >= 1 << 29 {
            return Ok(None);
        }
        let mut relocated = [0; 8];
        relocated[..4].copy_from_slice(&((offset as u32) << 2).to_le_bytes());
        relocated[4..].copy_from_slice(&target[4..8]);
        let mut header = [0; 16];
        header[4..8].copy_from_slice(&first_words.to_le_bytes());
        let header_len = if let Some(second) = self.second {
            header[..4].copy_from_slice(&1u32.to_le_bytes());
            let second_words = u32::try_from(second.len() / 8)
                .map_err(|_| super::super::super::Error::Capacity)?;
            header[8..12].copy_from_slice(&second_words.to_le_bytes());
            16
        } else {
            8
        };
        let lease = messages.copy_from_parts_with_minimum(
            &[
                &header[..header_len],
                self.segment,
                appended,
                self.second.unwrap_or(&[]),
            ],
            minimum,
            |output| {
                let body = &mut output[header_len..];
                body[self.template.call..self.template.call + 4]
                    .copy_from_slice(&question.to_wire().to_le_bytes());
                body[self.template.call + 16] = (body[self.template.call + 16] & !0b111)
                    | u8::from(allow_handoff)
                    | (u8::from(self.template.no_promise_pipelining) << 1);
                // MessageTarget.promisedAnswer is union discriminant 1 at
                // UInt16 slot 2; importedCap's inactive UInt32 slot is cleared.
                body[self.template.target..self.template.target + 4].fill(0);
                body[self.template.target + 4..self.template.target + 6]
                    .copy_from_slice(&1u16.to_le_bytes());
                body[pointer..pointer + 8].copy_from_slice(&relocated);
            },
        );
        let lease = match lease {
            Ok(lease) => lease,
            // Compacting with the general encoder can fit when a prepared
            // builder retains unused words from earlier parameter replacements.
            Err(messages::Error::NoBuffer) => return Ok(None),
            Err(error) => return Err(super::super::super::Error::Storage(error)),
        };
        Ok(Some(Frame::new(lease, options)))
    }
    /// A promised-answer target needs a transform list. Preserve certified
    /// segments when possible, falling back to copying the parameter graph.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn copy_promised<'pool>(
        &self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        parent: QuestionId,
        path: pipeline::Path<'_>,
        method: calls::DynamicCall,
        minimum: usize,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
        writer: WriterPolicy,
    ) -> Result<Frame<'pool>, super::super::super::Error> {
        // Certify the original immutable payload under this publication's
        // policy. Appending a target preserves every payload pointer, its
        // nesting depth and its envelope traversal charge. The append checks
        // the larger serialized word count independently, so walking the
        // unchanged payload again would provide no additional validation.
        let options = writer.options(options);
        self.for_publication(method, options, writer)?;
        if let Some(frame) = self.append_promised(
            messages,
            question,
            parent,
            path,
            minimum,
            allow_handoff,
            options,
        )? {
            return Ok(frame);
        }
        let segments = [self.segment, self.second.unwrap_or(&[])];
        let segments = &segments[..if self.second.is_some() { 2 } else { 1 }];
        let reader =
            capnp::message::Reader::new(capnp::message::SegmentArray::new(segments), options);
        let rpc_capnp::message::Call(call) = reader
            .get_root::<rpc_capnp::message::Reader>()?
            .which()
            .map_err(|_| super::super::super::Error::WrongMessage)?
        else {
            return Err(super::super::super::Error::WrongMessage);
        };
        let content = call?.get_params()?.get_content();
        let sized = content
            .target_size()?
            .word_count
            .checked_add(18)
            .and_then(|words| words.checked_add((path.fields().len() as u64).checked_mul(2)?))
            .and_then(|words| words.checked_mul(8))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(super::super::super::Error::Capacity)?;
        let frame = Frame::build(messages, minimum.max(sized), options, |builder| {
            calls::dynamic_call_with_handoff(
                builder,
                question,
                calls::Target::Promised {
                    question: parent,
                    path,
                },
                method,
                allow_handoff,
                |field| {
                    field.copy_with_capabilities(
                        content,
                        capnp::wire_capability::IndexMap::identity(0),
                    )
                },
            )
        })?;
        match writer {
            WriterPolicy::Checked => Self::validate_promised(frame, messages),
            WriterPolicy::Native => Ok(frame),
        }
    }
    fn validate_promised<'pool>(
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<Frame<'pool>, super::super::super::Error> {
        let validation = frame
            .with_borrowed_root(messages, |root| -> capnp::Result<()> {
                let rpc_capnp::message::Call(call) = root.which()? else {
                    unreachable!("built Call")
                };
                call?
                    .get_params()?
                    .get_content()
                    .validate_capability_indices(0)
            })
            .and_then(|result| result.map_err(super::super::super::Error::from));
        if let Err(error) = validation {
            frame.release(messages);
            return Err(error);
        }
        Ok(frame)
    }
    #[cfg(feature = "alloc")]
    pub fn from_builder<A: capnp::message::Allocator>(
        builder: &'a capnp::message::Builder<A>,
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Self>, super::super::super::Error> {
        let capnp::OutputSegments::SingleSegment([segment]) = builder.get_segments_for_output()
        else {
            return Ok(None);
        };
        Self::new(segment, method, options).map(Some)
    }
    pub fn from_fixed(
        builder: &'a capnp::message::FixedBuilder<'_>,
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Self, super::super::super::Error> {
        Self::new(builder.get_segments_for_output()[0], method, options)
    }
    fn new(
        segment: &'a [u8],
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Self, super::super::super::Error> {
        Ok(Self {
            segment,
            second: None,
            #[cfg(feature = "alloc")]
            owner: None,
            template: PlainTemplate::read_segment(segment, method, options)?,
            options: Some(options),
        })
    }
    #[cfg(test)]
    pub(super) fn segment(&self) -> &'a [u8] {
        self.segment
    }
    #[inline(always)]
    pub(super) fn for_publication(
        &self,
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
        writer: WriterPolicy,
    ) -> Result<PlainTemplate, super::super::super::Error> {
        if self.template.method != (method.interface, method.method)
            || self.template.no_promise_pipelining != method.no_promise_pipelining
            || method.only_promise_pipeline
        {
            return Err(super::super::super::Error::WrongMessage);
        }
        if writer == WriterPolicy::Native
            || self.options.is_some_and(|validated| {
                options.traversal_limit_in_words == validated.traversal_limit_in_words
                    && options.nesting_limit == validated.nesting_limit
            })
        {
            Ok(self.template)
        } else {
            // The private certificate already proves the immutable envelope's
            // method, target layout and empty capability table. Recheck only
            // the ordinary envelope/payload traversal under the new limits.
            let segments = [self.segment, self.second.unwrap_or(&[])];
            let segments = &segments[..if self.second.is_some() { 2 } else { 1 }];
            let words = self.segment.len() / 8 + self.second.map_or(0, |second| second.len() / 8);
            if options
                .traversal_limit_in_words
                .is_some_and(|limit| words > limit)
            {
                return Err(
                    capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(words)).into(),
                );
            }
            let reader =
                capnp::message::Reader::new(capnp::message::SegmentArray::new(segments), options);
            let root = reader.get_root::<rpc_capnp::message::Reader>()?;
            let rpc_capnp::message::Call(call) = root
                .which()
                .map_err(|_| super::super::super::Error::WrongMessage)?
            else {
                return Err(super::super::super::Error::WrongMessage);
            };
            call?
                .get_params()?
                .get_content()
                .validate_capability_indices(0)?;
            Ok(self.template)
        }
    }
    #[inline(always)]
    pub(super) fn copy<'pool>(
        &self,
        template: PlainTemplate,
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        target: ImportId,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
    ) -> Result<Frame<'pool>, super::super::super::Error> {
        let Some(second) = self.second else {
            return template.copy_segment(
                self.segment,
                messages,
                question,
                target,
                allow_handoff,
                options,
            );
        };
        let first_words = u32::try_from(self.segment.len() / 8)
            .map_err(|_| super::super::super::Error::Capacity)?;
        let second_words =
            u32::try_from(second.len() / 8).map_err(|_| super::super::super::Error::Capacity)?;
        let mut header = [0; 16];
        header[..4].copy_from_slice(&1u32.to_le_bytes());
        header[4..8].copy_from_slice(&first_words.to_le_bytes());
        header[8..12].copy_from_slice(&second_words.to_le_bytes());
        let lease = messages
            .copy_from_parts(&[&header, self.segment, second], |output| {
                // Both scalar fields belong to the immutable first-segment header.
                // Copying both segments preserves all far pointers and landing pads.
                let body = &mut output[16..];
                body[template.call..template.call + 4]
                    .copy_from_slice(&question.to_wire().to_le_bytes());
                body[template.target..template.target + 4]
                    .copy_from_slice(&target.to_wire().to_le_bytes());
                body[template.call + 16] = (body[template.call + 16] & !0b111)
                    | u8::from(allow_handoff)
                    | (u8::from(template.no_promise_pipelining) << 1);
            })
            .map_err(super::super::super::Error::Storage)?;
        Ok(Frame::new(lease, options))
    }
}

#[derive(Clone, Copy)]
pub(super) struct PlainTemplate {
    pub(super) method: (u64, u16),
    no_promise_pipelining: bool,
    pub(super) only_promise_pipeline: bool,
    call: usize,
    target: usize,
    target_data_words: Option<core::num::NonZeroU16>,
    length: usize,
}

impl PlainTemplate {
    fn read_segment(
        segment: &[u8],
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Self, super::super::super::Error> {
        let words = segment.len() / 8;
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| words > limit)
        {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(words)).into());
        }
        let segments = [segment];
        let reader =
            capnp::message::Reader::new(capnp::message::SegmentArray::new(&segments), options);
        let root = reader.get_root::<rpc_capnp::message::Reader>()?;
        let rpc_capnp::message::Call(call) = root
            .which()
            .map_err(|_| super::super::super::Error::WrongMessage)?
        else {
            return Err(super::super::super::Error::WrongMessage);
        };
        let call = call?;
        if call.get_interface_id() != method.interface
            || call.get_method_id() != method.method
            || call.get_no_promise_pipelining() != method.no_promise_pipelining
            || call.get_only_promise_pipeline()
            || method.only_promise_pipeline
        {
            return Err(super::super::super::Error::WrongMessage);
        }
        let template = Self::new(call, segment).ok_or(super::super::super::Error::WrongMessage)?;
        call.get_params()?
            .get_content()
            .validate_capability_indices(0)?;
        Ok(template)
    }
    #[inline(always)]
    pub(super) fn copy_segment<'pool>(
        self,
        segment: &[u8],
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        target: ImportId,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
    ) -> Result<Frame<'pool>, super::super::super::Error> {
        let words =
            u32::try_from(segment.len() / 8).map_err(|_| super::super::super::Error::Capacity)?;
        let mut header = [0; 8];
        header[4..].copy_from_slice(&words.to_le_bytes());
        let lease = messages
            .copy_from_parts(&[&header, segment], |output| {
                let body = &mut output[8..];
                body[self.call..self.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
                body[self.target..self.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
                body[self.call + 16] = (body[self.call + 16] & !0b111)
                    | u8::from(allow_handoff)
                    | (u8::from(self.no_promise_pipelining) << 1);
            })
            .map_err(super::super::super::Error::Storage)?;
        Ok(Frame::new(lease, options))
    }
    pub(super) fn new(call: rpc_capnp::call::Reader<'_>, bytes: &[u8]) -> Option<Self> {
        if !matches!(
            call.get_send_results_to().which().ok()?,
            rpc_capnp::call::send_results_to::Caller(())
        ) || !call.get_params().ok()?.get_cap_table().ok()?.is_empty()
        {
            return None;
        }
        let target = call.get_target().ok()?;
        if !matches!(
            target.which().ok()?,
            rpc_capnp::message_target::ImportedCap(_)
        ) {
            return None;
        }
        let offset = |data: &[u8], minimum: usize| {
            let start = (data.as_ptr() as usize).checked_sub(bytes.as_ptr() as usize)?;
            (data.len() >= minimum && start.checked_add(data.len())? <= bytes.len())
                .then_some(start)
        };
        let target = target.into_internal_struct_reader();
        let data = target.get_data_section_as_blob();
        let target_offset = offset(data, 8)?;
        let pointer = target_offset.checked_add(data.len())?;
        let target_data_words = (target.get_pointer_section_size() != 0
            && pointer.checked_add(8)? <= bytes.len())
        .then(|| core::num::NonZeroU16::new(u16::try_from(data.len() / 8).ok()?))
        .flatten();
        Some(Self {
            method: (call.get_interface_id(), call.get_method_id()),
            no_promise_pipelining: call.get_no_promise_pipelining(),
            only_promise_pipeline: call.get_only_promise_pipeline(),
            call: offset(
                call.into_internal_struct_reader()
                    .get_data_section_as_blob(),
                17,
            )?,
            target: target_offset,
            target_data_words,
            length: bytes.len(),
        })
    }
    pub(super) fn copy<'pool>(
        self,
        source: &Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        target: ImportId,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
    ) -> Result<Frame<'pool>, super::super::super::Error> {
        let mut writer = messages
            .reserve_exact(self.length)
            .map_err(super::super::super::Error::Storage)?;
        let result = (|| {
            let bytes = messages.read(&source.lease);
            let mut output = messages
                .write_borrowed(&mut writer)
                .map_err(super::super::super::Error::Storage)?;
            output.copy_from_slice(&bytes);
            // rpc.capnp scalar layout: Call.questionId is UInt32 slot 0;
            // MessageTarget.importedCap is UInt32 slot 0. Bits 128 and 130
            // are allowThirdPartyTailCall and onlyPromisePipeline respectively.
            // No pointer, discriminant, schema payload or capability is changed.
            output[self.call..self.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
            output[self.target..self.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
            let pipeline_only = self.only_promise_pipeline && question.is_pipeline_only();
            output[self.call + 16] = (output[self.call + 16] & !0b111)
                | u8::from(allow_handoff)
                | (u8::from(self.no_promise_pipelining && !pipeline_only) << 1)
                | (u8::from(pipeline_only) << 2);
            Ok(())
        })();
        if let Err(error) = result {
            messages.discard(writer);
            return Err(error);
        }
        match messages.seal(writer, self.length) {
            Ok(lease) => Ok(Frame::new(lease, options)),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(super::super::super::Error::Storage(rejected.error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::{field_api::Data, word, Word};

    #[cfg(feature = "alloc")]
    #[test]
    fn promised_template_matches_codec_for_near_far_and_long_paths() {
        fn canonical(root: rpc_capnp::message::Reader<'_>) -> alloc::vec::Vec<u8> {
            let mut message = capnp::message::Builder::new_default();
            message.set_root_canonical(root).unwrap();
            message.get_segments_for_output()[0].to_vec()
        }
        for length in [0, 13, 4096] {
            let mut call = PreparedCall::from_template(
                &PreparedCallTemplate::default(),
                capnp::message::HeapAllocator::new().first_segment_words(32),
                u64::MAX,
                u16::MAX,
            )
            .unwrap();
            let data = alloc::vec![0xa5; length];
            call.params()
                .set_as::<capnp::data::Owned>(&data[..])
                .unwrap();
            let original = capnp::serialize::write_message_to_words(call.message());
            for no_promise_pipelining in [false, true] {
                let method = calls::DynamicCall {
                    no_promise_pipelining,
                    ..call.method
                };
                let encoded = call.encoded(method, Default::default()).unwrap().unwrap();
                for fields in [&[][..], &[u16::MAX][..], &[17; 16][..], &[31; 17][..]] {
                    for allow_handoff in [false, true] {
                        let mut a = Word::allocate_zeroed_vec(1024);
                        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut a))];
                        messages::with_messages(&mut buffers, &mut [], |messages| {
                            let id = QuestionId::from_wire(u32::MAX);
                            let parent = QuestionId::from_wire(0x7654_3210);
                            let path = pipeline::Path::new(fields);
                            // Exhaustion must preserve the certificate and source.
                            let occupied = messages.reserve(128).unwrap();
                            assert!(encoded
                                .copy_promised(
                                    messages,
                                    id,
                                    parent,
                                    path,
                                    method,
                                    128,
                                    allow_handoff,
                                    Default::default(),
                                    WriterPolicy::Checked
                                )
                                .is_err());
                            messages.discard(occupied);
                            let appended = encoded
                                .append_promised(
                                    messages,
                                    id,
                                    parent,
                                    path,
                                    128,
                                    allow_handoff,
                                    Default::default(),
                                )
                                .unwrap();
                            assert_eq!(appended.is_some(), fields.len() <= 16);
                            if let Some(frame) = appended {
                                frame.release(messages);
                            }
                            let frame = encoded
                                .copy_promised(
                                    messages,
                                    id,
                                    parent,
                                    path,
                                    method,
                                    128,
                                    allow_handoff,
                                    Default::default(),
                                    WriterPolicy::Checked,
                                )
                                .unwrap();
                            let actual = frame.with(messages, canonical).unwrap();
                            frame.release(messages);
                            let expected =
                                Frame::build(messages, 8192, Default::default(), |builder| {
                                    calls::dynamic_call_with_handoff(
                                        builder,
                                        id,
                                        calls::Target::Promised {
                                            question: parent,
                                            path,
                                        },
                                        method,
                                        allow_handoff,
                                        |field| field.cast::<Data>().copy_from(&data),
                                    )
                                })
                                .unwrap();
                            assert_eq!(actual, expected.with(messages, canonical).unwrap());
                            expected.release(messages);
                            let strict = capnp::message::ReaderOptions {
                                nesting_limit: 3,
                                ..Default::default()
                            };
                            assert!(encoded
                                .copy_promised(
                                    messages,
                                    id,
                                    parent,
                                    path,
                                    method,
                                    128,
                                    allow_handoff,
                                    strict,
                                    WriterPolicy::Checked
                                )
                                .is_err());
                            // Rejected validation returns its buffer to the pool.
                            let retry = messages.reserve(8192).unwrap();
                            messages.discard(retry);
                        })
                        .unwrap();
                    }
                }
            }
            assert_eq!(
                original,
                capnp::serialize::write_message_to_words(call.message())
            );
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn promised_payload_certificate_preserves_nesting_and_traversal_limits() {
        for depth in [0, 1, 8] {
            let mut call =
                PreparedCall::new(capnp::message::Builder::new_default(), 17, 3).unwrap();
            let mut field = call.params();
            for _ in 0..depth {
                field = field.initn_as::<capnp::any_pointer_list::Builder>(1).get(0);
            }
            field.set_as::<capnp::data::Owned>(&b"payload"[..]).unwrap();
            let method = call.method;
            let encoded = call.encoded(method, Default::default()).unwrap().unwrap();
            let mut storage = Word::allocate_zeroed_vec(256);
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(
                &mut storage,
            ))];
            messages::with_messages(&mut buffers, &mut [], |messages| {
                for nesting_limit in 0..16 {
                    for traversal_limit_in_words in [None, Some(8), Some(16), Some(32), Some(64)] {
                        let options = capnp::message::ReaderOptions {
                            nesting_limit,
                            traversal_limit_in_words,
                        };
                        let question = QuestionId::from_wire(7);
                        let parent = QuestionId::from_wire(3);
                        let path = pipeline::Path::new(&[0]);
                        let Some(reference) = encoded.append_promised(
                            messages, question, parent, path, 128, false, options,
                        ).unwrap() else {
                            continue; // This case uses the unchanged general encoder.
                        };
                        let reference = EncodedCall::validate_promised(reference, messages);
                        let expected = reference.is_ok();
                        if let Ok(frame) = reference {
                            frame.release(messages);
                        }
                        let actual = encoded.copy_promised(
                            messages, question, parent, path, method, 128, false, options,
                         WriterPolicy::Checked);
                        assert_eq!(actual.is_ok(), expected,
                            "depth={depth}, nesting={nesting_limit}, words={traversal_limit_in_words:?}");
                        if let Ok(frame) = actual {
                            frame.release(messages);
                        }
                    }
                }
            }).unwrap();
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn prepared_pipeline_hints_preserve_source_and_reader_limits() {
        for length in [16, 4096] {
            let mut call = PreparedCall::new(
                capnp::message::Builder::new(
                    capnp::message::HeapAllocator::new().first_segment_words(32),
                ),
                17,
                3,
            )
            .unwrap();
            let data = alloc::vec![0xa5; length];
            call.params()
                .set_as::<capnp::data::Owned>(&data[..])
                .unwrap();
            for no_promise_pipelining in [false, true] {
                let method = calls::DynamicCall {
                    no_promise_pipelining,
                    ..call.method
                };
                let encoded = call.encoded(method, Default::default()).unwrap().unwrap();
                let mut changed = capnp::message::ReaderOptions::new();
                changed.nesting_limit -= 1;
                assert!(encoded
                    .for_publication(method, changed, WriterPolicy::Checked)
                    .is_ok());
                changed.nesting_limit = 0;
                assert!(encoded
                    .for_publication(method, changed, WriterPolicy::Checked)
                    .is_err());
                let wrong = calls::DynamicCall {
                    no_promise_pipelining: !no_promise_pipelining,
                    ..method
                };
                assert!(encoded
                    .for_publication(wrong, Default::default(), WriterPolicy::Checked)
                    .is_err());
                let mut words = Word::allocate_zeroed_vec(1024);
                let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
                messages::with_messages(&mut buffers, &mut [], |messages| {
                    let frame = encoded
                        .copy(
                            encoded.template,
                            messages,
                            QuestionId::from_wire(5),
                            ImportId::from_wire(7),
                            true,
                            Default::default(),
                        )
                        .unwrap();
                    frame
                        .with(messages, |root| {
                            let rpc_capnp::message::Call(value) = root.which().unwrap() else {
                                panic!()
                            };
                            let value = value.unwrap();
                            assert_eq!(value.get_no_promise_pipelining(), no_promise_pipelining);
                            assert!(!value.get_only_promise_pipeline());
                            assert_eq!(value.get_question_id(), 5);
                            assert!(value.get_allow_third_party_tail_call());
                            assert_eq!(
                                value
                                    .get_params()
                                    .unwrap()
                                    .get_content()
                                    .get_as::<capnp::data::Reader>()
                                    .unwrap(),
                                data
                            );
                        })
                        .unwrap();
                    frame.release(messages);
                })
                .unwrap();
            }
            let root = call
                .message()
                .get_root_as_reader::<rpc_capnp::message::Reader>()
                .unwrap();
            let rpc_capnp::message::Call(value) = root.which().unwrap() else {
                panic!()
            };
            assert!(value.unwrap().get_no_promise_pipelining());
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn two_segment_call_preserves_far_payload_and_revalidates_limits() {
        use capnp::message::{HeapAllocator, Reader, ReaderOptions, SegmentArray};
        let mut call = PreparedCall::from_template(
            &PreparedCallTemplate::default(),
            HeapAllocator::new().first_segment_words(32),
            17,
            3,
        )
        .unwrap();
        let data = [0xa5; 4096];
        call.params()
            .set_as::<capnp::data::Owned>(&data[..])
            .unwrap();
        let segments = call.message().get_segments_for_output();
        assert_eq!(segments.len(), 2);
        let words = segments
            .iter()
            .map(|segment| segment.len() / 8)
            .sum::<usize>();
        let encoded = call
            .encoded(call.method, ReaderOptions::new())
            .unwrap()
            .unwrap();
        assert!(encoded.second.is_some());
        for nesting_limit in -1..8 {
            for limit in [
                Some(0),
                Some(words - 1),
                Some(words),
                Some(words + 32),
                None,
            ] {
                let options = ReaderOptions {
                    nesting_limit,
                    traversal_limit_in_words: limit,
                };
                let expected = (|| -> capnp::Result<()> {
                    if limit.is_some_and(|limit| words > limit) {
                        return Err(capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(
                            words,
                        )));
                    }
                    let reader = Reader::new(SegmentArray::new(&segments), options);
                    let root = reader.get_root::<rpc_capnp::message::Reader>()?;
                    let rpc_capnp::message::Call(value) = root.which().unwrap() else {
                        panic!()
                    };
                    value?
                        .get_params()?
                        .get_content()
                        .validate_capability_indices(0)
                })()
                .map_err(crate::owned::Error::from);
                assert_eq!(call.encoded(call.method, options).map(|_| ()), expected);
                assert_eq!(
                    encoded
                        .for_publication(call.method, options, WriterPolicy::Checked)
                        .is_ok(),
                    expected.is_ok()
                );
            }
        }
        let mut output_words = [word(0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa); 1024];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(
            &mut output_words,
        ))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            for (id, handoff) in [(u32::MAX, true), (0, false)] {
                let frame = encoded
                    .copy(
                        encoded.template,
                        messages,
                        QuestionId::from_wire(id),
                        ImportId::from_wire(id),
                        handoff,
                        ReaderOptions::new(),
                    )
                    .unwrap();
                assert_eq!(&messages.read(&frame.lease)[12..16], &[0; 4]);
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Call(value) = root.which().unwrap() else {
                            panic!()
                        };
                        let value = value.unwrap();
                        assert_eq!(value.get_question_id(), id);
                        assert!(matches!(value.get_target().unwrap().which().unwrap(),
                        rpc_capnp::message_target::ImportedCap(target) if target == id));
                        assert_eq!((value.get_interface_id(), value.get_method_id()), (17, 3));
                        assert!(value.get_no_promise_pipelining());
                        assert_eq!(value.get_allow_third_party_tail_call(), handoff);
                        assert_eq!(
                            value
                                .get_params()
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            &data
                        );
                    })
                    .unwrap();
                frame.release(messages);
            }
        })
        .unwrap();
        assert_eq!(
            call.params().get_as::<capnp::data::Builder>().unwrap(),
            data
        );
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn retained_call_parts_match_copied_wire_bytes_and_preserve_source() {
        use capnp::message::{HeapAllocator, ReaderSegments};
        use capntproto_rpc_engine::connection::Growth;
        let mut call = PreparedCall::from_template(
            &PreparedCallTemplate::default(),
            HeapAllocator::new().first_segment_words(32),
            17,
            3,
        )
        .unwrap();
        let data = [0xa5; 8192];
        call.params()
            .set_as::<capnp::data::Owned>(&data[..])
            .unwrap();
        let call = call.into_shared();
        let source = capnp::serialize::write_message_to_words(call.message());
        let source_capacity = call.message().retained_segment_bytes().unwrap();
        assert!(source_capacity >= call.message().size_in_words() * 8);
        let mut pool = messages::GrowingBuffers::new(4, 16384, 65536);
        messages::with_growing_messages(
            &mut pool,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let encoded = call
                    .encoded(call.method, Default::default())
                    .unwrap()
                    .unwrap();
                for (id, handoff) in [(u32::MAX, true), (0, false)] {
                    let parts = encoded
                        .retain_parts(
                            encoded.template,
                            messages,
                            QuestionId::from_wire(id),
                            ImportId::from_wire(id),
                            handoff,
                        )
                        .unwrap()
                        .unwrap();
                    // The large payload is the original segment, not a copied buffer.
                    assert_eq!(
                        parts.part(2).as_ptr(),
                        call.message().get_segment(1).unwrap().as_ptr()
                    );
                    let expected = encoded
                        .copy(
                            encoded.template,
                            messages,
                            QuestionId::from_wire(id),
                            ImportId::from_wire(id),
                            handoff,
                            Default::default(),
                        )
                        .unwrap();
                    let actual: alloc::vec::Vec<_> = (0..parts.count())
                        .flat_map(|i| parts.part(i).iter().copied())
                        .collect();
                    assert_eq!(&actual, &*messages.read(&expected.lease));
                    expected.release(messages);
                    drop(parts);
                    assert_eq!(
                        capnp::serialize::write_message_to_words(call.message()),
                        source
                    );
                }
                let strict = capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(1),
                    ..Default::default()
                };
                assert!(encoded
                    .for_publication(call.method, strict, WriterPolicy::Checked)
                    .is_err());
            },
        )
        .unwrap();
        // Retaining original capacities may fail even where compact output fits.
        let mut small = messages::GrowingBuffers::new(1, 16384, source.len() + 7);
        messages::with_growing_messages(
            &mut small,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let encoded = call
                    .encoded(call.method, Default::default())
                    .unwrap()
                    .unwrap();
                assert!(encoded
                    .retain_parts(
                        encoded.template,
                        messages,
                        QuestionId::from_wire(1),
                        ImportId::from_wire(1),
                        false
                    )
                    .unwrap()
                    .is_none());
                let frame = encoded
                    .copy(
                        encoded.template,
                        messages,
                        QuestionId::from_wire(1),
                        ImportId::from_wire(1),
                        false,
                        Default::default(),
                    )
                    .unwrap();
                frame.release(messages);
            },
        )
        .unwrap();
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn native_prepared_call_defers_payload_limits_but_checked_publication_revalidates() {
        let mut call = PreparedCall::from_template(
            &PreparedCallTemplate::default(),
            capnp::message::HeapAllocator::new(),
            7,
            9,
        )
        .unwrap();
        let mut content = call.params();
        for _ in 0..16 {
            content = content
                .initn_as::<capnp::any_pointer_list::Builder>(1)
                .get(0);
        }
        content
            .set_as::<capnp::text::Owned>("deep native payload")
            .unwrap();
        let limits = capnp::message::ReaderOptions {
            nesting_limit: 4,
            ..Default::default()
        };
        assert!(call.encoded(call.method, limits).is_err());
        let encoded = call.encoded_for_sending(call.method).unwrap().unwrap();
        assert!(encoded
            .for_publication(call.method, limits, WriterPolicy::Checked)
            .is_err());
        assert!(encoded
            .for_publication(call.method, limits, WriterPolicy::Native)
            .is_ok());
        assert!(encoded
            .for_publication(
                calls::DynamicCall {
                    method: 10,
                    ..call.method
                },
                limits,
                WriterPolicy::Native
            )
            .is_err());
        let segments = [encoded.segment];
        let reader =
            capnp::message::Reader::new(capnp::message::SegmentArray::new(&segments), limits);
        let rpc_capnp::message::Call(received) = reader
            .get_root::<rpc_capnp::message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!("Call")
        };
        assert!(received
            .unwrap()
            .get_params()
            .unwrap()
            .get_content()
            .validate_capability_indices(0)
            .is_err());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn cached_call_validation_cannot_survive_parameter_mutation() {
        let mut call = PreparedCall::from_template(
            &PreparedCallTemplate::default(),
            capnp::message::HeapAllocator::new().first_segment_words(256),
            0,
            0,
        )
        .unwrap();
        let options = capnp::message::ReaderOptions {
            nesting_limit: 8,
            ..Default::default()
        };
        call.params()
            .set_as::<capnp::data::Owned>(b"small".as_slice())
            .unwrap();
        for _ in 0..3 {
            assert!(call.encoded(call.method, options).unwrap().is_some());
        }
        // Replacing the selected subtree leaves the private header unchanged.
        // A cached success for its old bytes must not authorize this deep value.
        let mut field = call.params();
        for _ in 0..16 {
            field = field.initn_as::<capnp::any_pointer_list::Builder>(1).get(0);
        }
        field
            .set_as::<capnp::data::Owned>(b"deep".as_slice())
            .unwrap();
        assert!(call.encoded(call.method, options).is_err());
        assert!(call
            .encoded(call.method, Default::default())
            .unwrap()
            .is_some());
        assert!(call.encoded(call.method, options).is_err());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn prepared_template_instantiates_independent_methods_and_parameters() {
        let template = PreparedCallTemplate::default();
        let mut first =
            PreparedCall::from_template(&template, capnp::message::HeapAllocator::new(), 7, 9)
                .unwrap();
        let mut second = PreparedCall::from_template(
            &template,
            capnp::message::HeapAllocator::new(),
            u64::MAX,
            u16::MAX,
        )
        .unwrap();
        drop(template);
        for (call, data) in [
            (&mut first, b"first".as_slice()),
            (&mut second, b"second".as_slice()),
        ] {
            call.params().set_as::<capnp::data::Owned>(data).unwrap();
            let encoded = call
                .encoded(call.method, Default::default())
                .unwrap()
                .unwrap();
            let checked =
                PlainTemplate::read_segment(encoded.segment, call.method, Default::default())
                    .unwrap();
            assert_eq!((checked.call, checked.target), call.offsets.unwrap());
        }
        assert_ne!(first.method, second.method);
        assert_eq!(
            first.params().get_as::<capnp::data::Builder>().unwrap(),
            b"first"
        );
        assert_eq!(
            second.params().get_as::<capnp::data::Builder>().unwrap(),
            b"second"
        );
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn selected_validation_matches_full_envelope_limits() {
        use capnp::message::{Builder, HeapAllocator, Reader, ReaderOptions, SegmentArray};
        for depth in [0, 1, 8] {
            for content in [None, Some(b"".as_slice()), Some(b"payload".as_slice())] {
                let mut call = PreparedCall::new(
                    Builder::new(HeapAllocator::new().first_segment_words(256)),
                    17,
                    3,
                )
                .unwrap();
                let mut field = call.params();
                for _ in 0..depth {
                    field = field.initn_as::<capnp::any_pointer_list::Builder>(1).get(0);
                }
                if let Some(bytes) = content {
                    field.set_as::<capnp::data::Owned>(bytes).unwrap();
                }
                let capnp::OutputSegments::SingleSegment([segment]) =
                    call.message().get_segments_for_output()
                else {
                    panic!("single segment");
                };
                let words = segment.len() / 8;
                let segments = [segment];
                for nesting_limit in -1..16 {
                    for limit in core::iter::once(None).chain((0..words + 20).map(Some)) {
                        let options = ReaderOptions {
                            traversal_limit_in_words: limit,
                            nesting_limit,
                        };
                        let expected = (|| -> capnp::Result<()> {
                            if limit.is_some_and(|limit| words > limit) {
                                return Err(capnp::Error::from_kind(
                                    capnp::ErrorKind::MessageTooLarge(words),
                                ));
                            }
                            let reader = Reader::new(SegmentArray::new(&segments), options);
                            let root = reader.get_root::<rpc_capnp::message::Reader>()?;
                            let rpc_capnp::message::Call(value) = root.which().unwrap() else {
                                panic!();
                            };
                            value?
                                .get_params()?
                                .get_content()
                                .validate_capability_indices(0)
                        })()
                        .map_err(crate::owned::Error::from);
                        assert_eq!(
                            call.encoded(call.method, options)
                                .map(|encoded| encoded.is_some()),
                            expected.map(|()| true),
                            "depth={depth}, content={content:?}, options={options:?}"
                        );
                    }
                }
            }
        }
    }

    #[cfg(all(feature = "alloc", not(miri)))]
    #[test]
    fn segmented_call_certification_borrows_segments_without_allocating() {
        use capnp::message::{HeapAllocator, ReaderOptions, ReaderSegments};
        let mut call = PreparedCall::from_template(
            &PreparedCallTemplate::default(),
            HeapAllocator::new().first_segment_words(32),
            0,
            0,
        )
        .unwrap();
        call.params()
            .set_as::<capnp::data::Owned>(&[7; 8192][..])
            .unwrap();
        assert_eq!(call.message().len(), 2);
        let measured = allocation_counter::measure(|| {
            let encoded = call
                .encoded(call.method, ReaderOptions::new())
                .unwrap()
                .unwrap();
            assert!(encoded.second.is_some());
            core::hint::black_box(encoded);
        });
        assert_eq!(measured.count_total, 0);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn prepared_call_preserves_header_across_parameter_replacement_and_policy_changes() {
        use capnp::message::{AllocationStrategy, Builder, HeapAllocator, ReaderOptions};
        for first_segment in [1, 256] {
            let mut call = PreparedCall::new(
                Builder::new(
                    HeapAllocator::new()
                        .first_segment_words(first_segment)
                        .allocation_strategy(AllocationStrategy::FixedSize),
                ),
                u64::MAX,
                u16::MAX,
            )
            .unwrap();
            let method = call.method;
            for bytes in [b"first".as_slice(), b"replaced parameters", b""] {
                call.params().set_as::<capnp::data::Owned>(bytes).unwrap();
                let root = call
                    .message()
                    .get_root_as_reader::<rpc_capnp::message::Reader>()
                    .unwrap();
                let rpc_capnp::message::Call(value) = root.which().unwrap() else {
                    panic!()
                };
                let value = value.unwrap();
                assert_eq!(value.get_interface_id(), u64::MAX);
                assert_eq!(value.get_method_id(), u16::MAX);
                assert!(value.get_no_promise_pipelining());
                assert!(!value.get_only_promise_pipeline());
                assert_eq!(
                    value
                        .get_params()
                        .unwrap()
                        .get_content()
                        .get_as::<capnp::data::Reader>()
                        .unwrap(),
                    bytes
                );
                let encoded = call.encoded(method, ReaderOptions::new()).unwrap();
                assert_eq!(encoded.is_some(), first_segment == 256);
                if let Some(encoded) = encoded {
                    assert!(encoded
                        .for_publication(
                            method,
                            ReaderOptions {
                                nesting_limit: 1,
                                ..ReaderOptions::new()
                            },
                            WriterPolicy::Checked
                        )
                        .is_err());
                    assert!(call
                        .encoded(
                            method,
                            ReaderOptions {
                                traversal_limit_in_words: Some(0),
                                ..ReaderOptions::new()
                            }
                        )
                        .is_err());
                }
            }
            assert!(call
                .encoded(
                    calls::DynamicCall {
                        method: 1,
                        ..method
                    },
                    ReaderOptions::new()
                )
                .is_err());
            // Extracting the full builder consumes the proof; arbitrary header
            // edits must pass through from_builder validation again.
            let mut message = call.into_inner();
            let rpc_capnp::message::Call(value) = message
                .get_root::<rpc_capnp::message::Builder>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            value.unwrap().set_method_id(1);
            if first_segment == 256 {
                assert!(EncodedCall::from_builder(&message, method, ReaderOptions::new()).is_err());
            }
        }
    }

    #[test]
    fn encoded_call_preserves_unknown_payload_fields_and_revalidates_reader_policy() {
        const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);
        let method = calls::DynamicCall {
            interface: u64::MAX,
            method: u16::MAX,
            no_promise_pipelining: true,
            only_promise_pipeline: false,
        };
        for length in [0, 1, 73, 1024] {
            let mut source_words = [ZERO; 256];
            let mut source = capnp::message::FixedBuilder::new(&mut source_words).unwrap();
            let data = [0xa5; 1024];
            calls::dynamic_call(
                &mut source,
                QuestionId::from_wire(99),
                calls::Target::Imported(ImportId::from_wire(77)),
                method,
                |field| field.cast::<Data>().copy_from(&data[..length]),
            )
            .unwrap();
            let encoded = EncodedCall::from_fixed(&source, method, Default::default()).unwrap();
            assert!(encoded
                .for_publication(
                    calls::DynamicCall {
                        method: 0,
                        ..method
                    },
                    Default::default(),
                    WriterPolicy::Checked
                )
                .is_err());
            assert!(encoded
                .for_publication(
                    method,
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: Some(0),
                        ..Default::default()
                    },
                    WriterPolicy::Checked
                )
                .is_err());
            let mut output_words = [ZERO; 256];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(
                &mut output_words,
            ))];
            messages::with_messages(&mut buffers, &mut [], |messages| {
                let occupied = messages.reserve(128).unwrap();
                assert!(encoded
                    .template
                    .copy_segment(
                        encoded.segment(),
                        messages,
                        QuestionId::from_wire(u32::MAX),
                        ImportId::from_wire(u32::MAX),
                        true,
                        Default::default()
                    )
                    .is_err());
                messages.discard(occupied);
                let frame = encoded
                    .template
                    .copy_segment(
                        encoded.segment(),
                        messages,
                        QuestionId::from_wire(u32::MAX),
                        ImportId::from_wire(u32::MAX),
                        true,
                        Default::default(),
                    )
                    .unwrap();
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                            panic!()
                        };
                        let call = call.unwrap();
                        assert_eq!(call.get_question_id(), u32::MAX);
                        assert!(matches!(
                            call.get_target().unwrap().which().unwrap(),
                            rpc_capnp::message_target::ImportedCap(u32::MAX)
                        ));
                        assert_eq!(
                            (call.get_interface_id(), call.get_method_id()),
                            (u64::MAX, u16::MAX)
                        );
                        assert!(
                            call.get_no_promise_pipelining()
                                && call.get_allow_third_party_tail_call()
                        );
                        assert_eq!(
                            call.get_params()
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            &data[..length]
                        );
                    })
                    .unwrap();
                frame.release(messages);
            })
            .unwrap();
            let source_root = source
                .get_root_as_reader::<rpc_capnp::message::Reader>()
                .unwrap();
            let rpc_capnp::message::Call(call) = source_root.which().unwrap() else {
                panic!()
            };
            assert_eq!(call.unwrap().get_question_id(), 99);
        }
        let mut source_words = [ZERO; 32];
        let mut source = capnp::message::FixedBuilder::new(&mut source_words).unwrap();
        calls::dynamic_call(
            &mut source,
            QuestionId::from_wire(0),
            calls::Target::Imported(ImportId::from_wire(0)),
            method,
            |field| {
                field
                    .cast::<capnp::field_api::WireCapability<()>>()
                    .set_index(0)
            },
        )
        .unwrap();
        assert!(EncodedCall::from_fixed(&source, method, Default::default()).is_err());
    }

    #[test]
    fn copied_template_matches_generated_encoding_and_preserves_source_on_exhaustion() {
        const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);
        for flags in 0..8 {
            for id in [0, 17, (1 << 31) - 1, 1 << 31, u32::MAX - 1, u32::MAX] {
                let mut a = [ZERO; 256];
                let mut b = [ZERO; 256];
                let mut c = [ZERO; 256];
                let mut buffers = [
                    messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
                    messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
                    messages::Buffer::new(Word::words_to_bytes_mut(&mut c)),
                ];
                messages::with_messages(&mut buffers, &mut [], |messages| {
                    let method = calls::DynamicCall {
                        interface: 0xfedc_ba98_7654_3210,
                        method: 0x87fe,
                        no_promise_pipelining: flags & 1 != 0,
                        only_promise_pipeline: flags & 2 != 0,
                    };
                    let payload = [0xa5; 73];
                    let source = Frame::build(messages, 512, Default::default(), |builder| {
                        calls::dynamic_call(
                            builder,
                            QuestionId::from_wire(0),
                            calls::Target::Imported(ImportId::from_wire(0)),
                            method,
                            |field| field.cast::<Data>().copy_from(&payload),
                        )
                    })
                    .unwrap();
                    let template = source
                        .with(messages, |root| {
                            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                                panic!()
                            };
                            PlainTemplate::new(call.unwrap(), &messages.read(&source.lease))
                                .unwrap()
                        })
                        .unwrap();
                    let allow = flags & 4 != 0;
                    let question = QuestionId::from_wire(id);
                    let target = ImportId::from_wire(!id);
                    let pipeline_only = method.only_promise_pipeline && question.is_pipeline_only();
                    let expected = Frame::build(messages, 512, Default::default(), |builder| {
                        calls::dynamic_call(
                            builder,
                            question,
                            calls::Target::Imported(target),
                            calls::DynamicCall {
                                only_promise_pipeline: pipeline_only,
                                no_promise_pipelining: method.no_promise_pipelining
                                    && !pipeline_only,
                                ..method
                            },
                            |field| field.cast::<Data>().copy_from(&payload),
                        )?;
                        builder
                            .try_get_root::<Message>()?
                            .field(Message::CALL)
                            .edit()?
                            .scalar(WireCall::ALLOW_THIRD_PARTY_TAIL_CALL)
                            .set(allow);
                        Ok(())
                    })
                    .unwrap();
                    let occupied = messages.reserve(512).unwrap();
                    assert!(matches!(
                        template.copy(
                            &source,
                            messages,
                            question,
                            target,
                            allow,
                            Default::default()
                        ),
                        Err(super::super::super::super::Error::Storage(
                            messages::Error::NoBuffer
                        ))
                    ));
                    source
                        .with(messages, |root| {
                            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                                panic!()
                            };
                            let call = call.unwrap();
                            assert_eq!(call.get_question_id(), 0);
                            assert_eq!(
                                call.get_only_promise_pipeline(),
                                method.only_promise_pipeline
                            );
                            assert!(!call.get_allow_third_party_tail_call());
                        })
                        .unwrap();
                    messages.discard(occupied);
                    let actual = template
                        .copy(
                            &source,
                            messages,
                            question,
                            target,
                            allow,
                            Default::default(),
                        )
                        .unwrap();
                    assert_eq!(
                        &*messages.read(&actual.lease),
                        &*messages.read(&expected.lease)
                    );
                    actual.release(messages);
                    expected.release(messages);
                    source.release(messages);
                    for _ in 0..3 {
                        let writer = messages.reserve(512).unwrap();
                        messages.discard(writer);
                    }
                })
                .unwrap();
            }
        }
    }
}
