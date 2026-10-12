//! Relocate a validated, data-only Call template without rebuilding its graph.
//! Relative pointers remain valid when the entire frame is copied. Only the
//! two scalar IDs and advisory bits change. The private publication path uses
//! the original validation certificate; changed reader limits revalidate it.
use super::*;
use capnp::traits::IntoInternalStructReader;
use capntproto_rpc_engine::ids::QuestionId;

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
/// application payload is still checked under the receiving driver's limits.
#[cfg(feature = "alloc")]
pub struct PreparedCall<A: capnp::message::Allocator> {
    message: capnp::message::SelectedBuilder<A>,
    method: calls::DynamicCall,
    offsets: Option<(usize, usize)>,
}
#[cfg(feature = "alloc")]
impl<A: capnp::message::Allocator> PreparedCall<A> {
    pub fn from_template(
        template: &PreparedCallTemplate,
        allocator: A,
        interface: u64,
        method: u16,
    ) -> capnp::Result<Self> {
        let message = template.message.instantiate(allocator);
        let message = if template.method == (interface, method) {
            message
        } else {
            message.into_inner().select(|root| {
                let root = root.get_as::<rpc_capnp::message::Builder>()?;
                let rpc_capnp::message::Call(call) =
                    root.which().expect("private Call discriminant")
                else {
                    unreachable!("private Call envelope")
                };
                let mut call = call?;
                call.set_interface_id(interface);
                call.set_method_id(method);
                Ok(call.get_params()?.get_content())
            })?
        };
        Ok(Self {
            message,
            offsets: template.offsets,
            method: calls::DynamicCall {
                interface,
                method,
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            },
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
            method: calls::DynamicCall {
                interface,
                method,
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            },
        })
    }
    pub fn params(&mut self) -> capnp::any_pointer::Builder<'_> {
        self.message.get()
    }
    pub fn message(&self) -> &capnp::message::Builder<A> {
        self.message.message()
    }
    pub fn into_inner(self) -> capnp::message::Builder<A> {
        self.message.into_inner()
    }
    /// Return a single-segment data-only certificate, or let the general codec
    /// handle multiple segments. Any capability pointer in the content fails
    /// validation; callers with a capability table must use the general codec.
    pub fn encoded(
        &self,
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<EncodedCall<'_>>, super::super::super::Error> {
        if self.method != method {
            return Err(super::super::super::Error::WrongMessage);
        }
        let Some((call, target)) = self.offsets else {
            return Ok(None);
        };
        let capnp::OutputSegments::SingleSegment([segment]) =
            self.message().get_segments_for_output()
        else {
            return Ok(None);
        };
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| segment.len() / 8 > limit)
        {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(
                segment.len() / 8,
            ))
            .into());
        }
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
                capnp::Error::from_kind(capnp::ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles)
            })?;
        if let Some(limit) = &mut selected_options.traversal_limit_in_words {
            *limit = limit
                .checked_sub(prefix_words)
                .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::ReadLimitExceeded))?;
        }
        self.message
            .validate_capability_indices(0, selected_options)?;
        Ok(Some(EncodedCall {
            segment,
            method,
            options,
            template: PlainTemplate {
                method: (method.interface, method.method),
                call,
                target,
                length: segment.len(),
            },
        }))
    }
}

/// An immutable single-segment Call constructed by a safe message builder.
/// Builder ownership excludes overlapping wire objects; changing scalar IDs
/// therefore cannot change parameters, pointers, or capability descriptors.
/// Raw wire bytes cannot construct this certificate.
pub struct EncodedCall<'a> {
    segment: &'a [u8],
    template: PlainTemplate,
    method: calls::DynamicCall,
    options: capnp::message::ReaderOptions,
}
impl<'a> EncodedCall<'a> {
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
            template: PlainTemplate::read_segment(segment, method, options)?,
            method,
            options,
        })
    }
    pub(super) fn segment(&self) -> &'a [u8] {
        self.segment
    }
    pub(super) fn for_policy(
        &self,
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<PlainTemplate, super::super::super::Error> {
        if self.method != method {
            return Err(super::super::super::Error::WrongMessage);
        }
        if options.traversal_limit_in_words == self.options.traversal_limit_in_words
            && options.nesting_limit == self.options.nesting_limit
        {
            Ok(self.template)
        } else {
            PlainTemplate::read_segment(self.segment, method, options)
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct PlainTemplate {
    pub(super) method: (u64, u16),
    call: usize,
    target: usize,
    length: usize,
}

impl PlainTemplate {
    fn read_segment(
        segment: &[u8],
        method: calls::DynamicCall,
        options: capnp::message::ReaderOptions,
    ) -> Result<Self, super::super::super::Error> {
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| segment.len() / 8 > limit)
        {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(
                segment.len() / 8,
            ))
            .into());
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
            || !call.get_no_promise_pipelining()
            || call.get_only_promise_pipeline()
            || !method.no_promise_pipelining
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
    pub(super) fn copy_segment<'pool>(
        self,
        segment: &[u8],
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        target: ImportId,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
    ) -> Result<Frame<'pool>, super::super::super::Error> {
        let length = segment
            .len()
            .checked_add(8)
            .ok_or(super::super::super::Error::Capacity)?;
        let words =
            u32::try_from(segment.len() / 8).map_err(|_| super::super::super::Error::Capacity)?;
        let mut writer = messages
            .reserve_exact(length)
            .map_err(super::super::super::Error::Storage)?;
        let result = (|| {
            let mut output = messages
                .write_borrowed(&mut writer)
                .map_err(super::super::super::Error::Storage)?;
            output[..4].copy_from_slice(&0u32.to_le_bytes());
            output[4..8].copy_from_slice(&words.to_le_bytes());
            output[8..].copy_from_slice(segment);
            let body = &mut output[8..];
            body[self.call..self.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
            body[self.target..self.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
            body[self.call + 16] = (body[self.call + 16] & !0b101) | u8::from(allow_handoff);
            Ok(())
        })();
        if let Err(error) = result {
            messages.discard(writer);
            return Err(error);
        }
        match messages.seal(writer, length) {
            Ok(lease) => Ok(Frame::new(lease, options)),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(super::super::super::Error::Storage(rejected.error))
            }
        }
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
        Some(Self {
            method: (call.get_interface_id(), call.get_method_id()),
            call: offset(
                call.into_internal_struct_reader()
                    .get_data_section_as_blob(),
                17,
            )?,
            target: offset(
                target
                    .into_internal_struct_reader()
                    .get_data_section_as_blob(),
                8,
            )?,
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
            output[self.call + 16] = (output[self.call + 16] & !0b101) | u8::from(allow_handoff);
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
                        .for_policy(
                            method,
                            ReaderOptions {
                                nesting_limit: 1,
                                ..ReaderOptions::new()
                            }
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
                .for_policy(
                    calls::DynamicCall {
                        method: 0,
                        ..method
                    },
                    Default::default()
                )
                .is_err());
            assert!(encoded
                .for_policy(
                    method,
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: Some(0),
                        ..Default::default()
                    }
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
            for id in [0, 17, u32::MAX] {
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
                    let expected = Frame::build(messages, 512, Default::default(), |builder| {
                        calls::dynamic_call(
                            builder,
                            question,
                            calls::Target::Imported(target),
                            calls::DynamicCall {
                                only_promise_pipeline: false,
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
