//! Hosted result envelopes whose ancestors cannot be edited by the application.
//! Data-only replies can be copied or retained without rebuilding pointers or
//! flattening their segments. Far pointers keep the same segment IDs and offsets.
use super::*;
use capnp::{field_api::Schema, traits::IntoInternalStructReader};

/// An immutable empty Return header. Each instance owns independent storage.
pub struct PreparedReturnTemplate {
    message: capnp::message::SelectedBuilderTemplate<capnp::message::HeapAllocator>,
    returned: usize,
}
impl PreparedReturnTemplate {
    /// Ancestor words to add to a payload hint; excludes the message root word.
    pub const ENVELOPE_WORDS: u64 = (rpc_capnp::api::Message::SIZE.data
        + rpc_capnp::api::Message::SIZE.pointers
        + rpc_capnp::api::Return::SIZE.data
        + rpc_capnp::api::Return::SIZE.pointers
        + rpc_capnp::api::Payload::SIZE.data
        + rpc_capnp::api::Payload::SIZE.pointers) as u64;
}
impl Default for PreparedReturnTemplate {
    fn default() -> Self {
        let mut returned = 0;
        let message = capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(16),
        )
        .select(|root| {
            let mut result = root.init_as::<rpc_capnp::message::Builder>().init_return();
            result.set_release_param_caps(false);
            returned = result
                .reborrow_as_reader()
                .into_internal_struct_reader()
                .get_data_section_as_blob()
                .as_ptr() as usize;
            Ok(result.init_results().get_content())
        })
        .expect("empty Return envelope");
        let segments = message.message().get_segments_for_output();
        let capnp::OutputSegments::SingleSegment([segment]) = segments else {
            unreachable!("empty Return fits its first segment")
        };
        returned -= segment.as_ptr() as usize;
        Self {
            message: message.into_template().expect("data-only Return header"),
            returned,
        }
    }
}

/// A private Return header with application-mutable result content. It carries
/// no answer authority; only the driver's live answer token permits publication.
pub struct PreparedReturn<A: capnp::message::Allocator> {
    message: capnp::message::SelectedBuilder<A>,
    returned: usize,
}
impl<A: capnp::message::Allocator> PreparedReturn<A> {
    pub fn from_template(template: &PreparedReturnTemplate, allocator: A) -> Self {
        Self {
            message: template.message.instantiate(allocator),
            returned: template.returned,
        }
    }
    pub fn results(&mut self) -> capnp::any_pointer::Builder<'_> {
        self.message.get()
    }
    /// Exact framed length when the private envelope's segment count fits the
    /// wire reader. This is a storage hint, not a validation certificate.
    pub fn framed_len(&self) -> Option<usize> {
        frame_layout(&self.message.message().get_segments_for_output()).map(|(_, length)| length)
    }
    pub fn reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        // Like RpcServerResponseImpl::getResultsBuilder().asReader(), use the
        // selected result field instead of decoding the envelope on each read.
        self.message.get_reader()
    }
    #[inline(always)]
    fn copy<'pool>(
        &self,
        answer: AnswerId,
        no_finish: bool,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'pool>>, super::super::Error> {
        self.copy_with_capabilities(answer, no_finish, 0, messages, options)
    }
    #[inline(always)]
    fn copy_with_capabilities<'pool>(
        &self,
        answer: AnswerId,
        no_finish: bool,
        capabilities: u32,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'pool>>, super::super::Error> {
        let segments = self.message.message().get_segments_for_output();
        let Some((header, length)) = frame_layout(&segments) else {
            return Ok(None);
        };
        let words = (length - header) / 8;
        self.validate(capabilities, words, options)?;
        self.copy_validated(
            answer, no_finish, messages, options, &segments, header, length,
        )
    }
    #[inline(always)]
    fn validate(
        &self,
        capabilities: u32,
        words: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<(), super::super::Error> {
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| words > limit)
        {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::MessageTooLarge(words)).into());
        }
        // Charge all three immutable ancestor structs, then validate the selected
        // pointer and its entire subtree using ordinary reader bounds and limits.
        let prefix_words = [
            rpc_capnp::api::Message::SIZE,
            rpc_capnp::api::Return::SIZE,
            rpc_capnp::api::Payload::SIZE,
        ]
        .iter()
        .map(|size| usize::from(size.data) + usize::from(size.pointers))
        .sum::<usize>();
        let mut selected = options;
        selected.nesting_limit = options
            .nesting_limit
            .checked_sub(3)
            .filter(|n| *n >= 0)
            .ok_or_else(|| {
                capnp::Error::from_kind(capnp::ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles)
            })?;
        if let Some(limit) = &mut selected.traversal_limit_in_words {
            *limit = limit
                .checked_sub(prefix_words)
                .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::ReadLimitExceeded))?;
        }
        self.message
            .validate_capability_indices(capabilities, selected)?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn copy_validated<'pool>(
        &self,
        answer: AnswerId,
        no_finish: bool,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
        segments: &[&[u8]],
        header: usize,
        length: usize,
    ) -> Result<Option<Frame<'pool>>, super::super::Error> {
        if let [segment] = segments {
            let mut prefix = [0; 8];
            prefix[4..].copy_from_slice(&((segment.len() / 8) as u32).to_le_bytes());
            let lease = messages
                .copy_from_parts(&[&prefix, segment], |bytes| {
                    let returned = self.returned + 8;
                    bytes[returned..returned + 4].copy_from_slice(&answer.to_wire().to_le_bytes());
                    bytes[returned + 4] = (bytes[returned + 4] & !2) | (u8::from(no_finish) << 1);
                })
                .map_err(super::super::Error::Storage)?;
            return Ok(Some(Frame::new(lease, options)));
        }
        if let [first, second] = segments {
            // Large Data/Text results commonly spill from the envelope into a
            // second segment. Overwrite the complete frame transactionally,
            // just like the single-segment path, instead of clearing a large
            // reusable buffer immediately before copying every byte into it.
            // The table includes its required zero padding; far pointers keep
            // their original segment IDs and offsets.
            let mut prefix = [0; 16];
            prefix[..4].copy_from_slice(&1u32.to_le_bytes());
            prefix[4..8].copy_from_slice(&((first.len() / 8) as u32).to_le_bytes());
            prefix[8..12].copy_from_slice(&((second.len() / 8) as u32).to_le_bytes());
            let lease = messages
                .copy_from_parts(&[&prefix, first, second], |bytes| {
                    let returned = self.returned + 16;
                    bytes[returned..returned + 4].copy_from_slice(&answer.to_wire().to_le_bytes());
                    bytes[returned + 4] = (bytes[returned + 4] & !2) | (u8::from(no_finish) << 1);
                })
                .map_err(super::super::Error::Storage)?;
            return Ok(Some(Frame::new(lease, options)));
        }
        let mut writer = messages
            .reserve_exact(length)
            .map_err(super::super::Error::Storage)?;
        let copied = (|| {
            let mut bytes = messages
                .write_borrowed(&mut writer)
                .map_err(super::super::Error::Storage)?;
            bytes[..4].copy_from_slice(&((segments.len() - 1) as u32).to_le_bytes());
            let mut start = header;
            for (index, segment) in segments.iter().enumerate() {
                let slot = (index + 1) * 4;
                bytes[slot..slot + 4].copy_from_slice(&((segment.len() / 8) as u32).to_le_bytes());
                let end = start + segment.len();
                bytes[start..end].copy_from_slice(segment);
                start = end;
            }
            bytes[(segments.len() + 1) * 4..header].fill(0);
            // rpc.capnp: answerId is UInt32 slot 0; noFinishNeeded is bit 33.
            // The safe private builder ensures these scalars cannot overlap
            // pointers or application content. releaseParamCaps remains false.
            let returned = self.returned + header;
            bytes[returned..returned + 4].copy_from_slice(&answer.to_wire().to_le_bytes());
            bytes[returned + 4] = (bytes[returned + 4] & !2) | (u8::from(no_finish) << 1);
            Ok(())
        })();
        if let Err(error) = copied {
            messages.discard(writer);
            return Err(error);
        }
        match messages.seal(writer, length) {
            Ok(lease) => Ok(Some(Frame::new(lease, options))),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(super::super::Error::Storage(rejected.error))
            }
        }
    }
}

struct FramedReturnParts {
    header: [u8; 16],
    first: alloc::vec::Vec<capnp::Word>,
    source: alloc::rc::Rc<PreparedReturn<capnp::message::HeapAllocator>>,
    capacity: usize,
}
impl messages::Parts for FramedReturnParts {
    fn count(&self) -> usize {
        3
    }
    fn part(&self, index: usize) -> &[u8] {
        use capnp::message::ReaderSegments;
        match index {
            0 => &self.header,
            1 => capnp::Word::words_to_bytes(&self.first),
            2 => self
                .source
                .message
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
impl PreparedReturn<capnp::message::HeapAllocator> {
    fn retain_parts(
        self: &alloc::rc::Rc<Self>,
        answer: AnswerId,
        no_finish: bool,
        messages: &mut messages::Messages<'_, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<messages::SharedParts>, super::super::Error> {
        use capnp::message::ReaderSegments;
        let message = self.message.message();
        if message.len() != 2 {
            return Ok(None);
        }
        let first = message.get_segment(0).expect("first segment");
        let second = message.get_segment(1).expect("second segment");
        if first.len() > 4096 || second.len() < 4096 {
            return Ok(None);
        }
        let Some(source_capacity) = message.retained_segment_bytes() else {
            return Ok(None);
        };
        let Some((header_len, length)) = frame_layout(&[first, second]) else {
            return Ok(None);
        };
        self.validate(0, (length - header_len) / 8, options)?;
        let mut header = [0; 16];
        header[..4].copy_from_slice(&1u32.to_le_bytes());
        header[4..8].copy_from_slice(&((first.len() / 8) as u32).to_le_bytes());
        header[8..12].copy_from_slice(&((second.len() / 8) as u32).to_le_bytes());
        let mut copied = capnp::Word::allocate_zeroed_vec(first.len() / 8);
        let bytes = capnp::Word::words_to_bytes_mut(&mut copied);
        bytes.copy_from_slice(first);
        bytes[self.returned..self.returned + 4].copy_from_slice(&answer.to_wire().to_le_bytes());
        bytes[self.returned + 4] = (bytes[self.returned + 4] & !2) | (u8::from(no_finish) << 1);
        let capacity = copied
            .capacity()
            .checked_mul(8)
            .and_then(|n| n.checked_add(16))
            .and_then(|n| n.checked_add(source_capacity))
            .ok_or(super::super::Error::Capacity)?;
        let source = alloc::rc::Rc::new(FramedReturnParts {
            header,
            first: copied,
            source: self.clone(),
            capacity,
        });
        match messages.retain_parts(source) {
            Ok(parts) => Ok(parts),
            // Compact reusable storage may still fit when retained capacities
            // exceed the budget. Its ordinary copy preserves retry semantics.
            Err(messages::Error::NoBuffer) => Ok(None),
            Err(error) => Err(super::super::Error::Storage(error)),
        }
    }
}

fn frame_layout(segments: &[&[u8]]) -> Option<(usize, usize)> {
    if segments.is_empty() || segments.len() >= capnp::serialize::SEGMENTS_COUNT_LIMIT {
        return None;
    }
    let header = (segments.len() / 2 + 1) * 8;
    let length = segments.iter().try_fold(header, |length, segment| {
        u32::try_from(segment.len() / 8).ok()?;
        length.checked_add(segment.len())
    })?;
    Some((header, length))
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Publish a data-only result directly when its answer has no pipeline or
    /// handoff dependencies. Validation and output reservation precede the
    /// ordinary Return transition; unavailable fast paths leave the answer
    /// untouched. The caller must still release its running request.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub fn try_send_answer_prepared<B: capnp::message::Allocator, R>(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        source: &PreparedReturn<B>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        options: capnp::message::ReaderOptions,
        resources: R,
    ) -> Result<Option<A>, Error> {
        self.try_send_answer_prepared_using(
            request, source, None, messages, queue, options, resources,
        )
    }

    /// Publish an immutable shared result, retaining large payload segments
    /// through hosted output. Fixed storage and small replies use the same copy
    /// path; answer authority and validation are unchanged.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub fn try_send_answer_prepared_shared<R>(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        source: &alloc::rc::Rc<PreparedReturn<capnp::message::HeapAllocator>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        options: capnp::message::ReaderOptions,
        resources: R,
    ) -> Result<Option<A>, Error> {
        self.try_send_answer_prepared_using(
            request,
            source,
            Some(source),
            messages,
            queue,
            options,
            resources,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn try_send_answer_prepared_using<B: capnp::message::Allocator, R>(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        source: &PreparedReturn<B>,
        shared: Option<&alloc::rc::Rc<PreparedReturn<capnp::message::HeapAllocator>>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        options: capnp::message::ReaderOptions,
        resources: R,
    ) -> Result<Option<A>, Error> {
        let key = request.answer();
        self.check_answer_completion(key)?;
        let mut pending = self
            .ledgers
            .answers
            .prepare_return(key)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
        if !pending.can_retire_without_finish() {
            return Ok(None);
        }
        let output = queue
            .reserve_with(resources)
            .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
        pending
            .omit_finish()
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
        if self.owned_output {
            if let Some(shared) = shared {
                if let Some(parts) = shared
                    .retain_parts(key.id(), true, messages, options)
                    .map_err(Error::Input)?
                {
                    let retired = pending
                        .accept()
                        .expect("no pipeline retains the omitted-Finish answer");
                    output.accept_parts(parts);
                    return Ok(Some(
                        retired
                            .release(messages)
                            .release(messages, &mut self.runtime),
                    ));
                }
            }
        }
        let Some(frame) = source
            .copy(key.id(), true, messages, options)
            .map_err(Error::Input)?
        else {
            return Ok(None);
        };
        let retired = if self.owned_output {
            let retired = pending
                .accept()
                .expect("no pipeline retains the omitted-Finish answer");
            output.accept_owned(frame.lease);
            retired
        } else {
            let offer = frame.offer(messages).map_err(Error::Input)?;
            let retired = pending
                .accept()
                .expect("no pipeline retains the omitted-Finish answer");
            output.accept(messages, offer);
            retired
        };
        Ok(Some(
            retired
                .release(messages)
                .release(messages, &mut self.runtime),
        ))
    }

    /// Prepare a data-only hosted answer without walking and rebuilding its
    /// envelope. None requests the normal codec to compact too many segments.
    /// Capability indices, traversal/nesting limits and publication ownership
    /// use the same checks as ordinary local answers. This does not send bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn local_answer_prepared<B: capnp::message::Allocator>(
        &mut self,
        answer: answers::Key<'id>,
        interface: u64,
        method: u16,
        source: &PreparedReturn<B>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<LocalResult<'pool, 'id, CAPS>>, Error> {
        self.check_live()?;
        let no_finish = self
            .ledgers
            .answers
            .prepare_return(answer)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
            .can_omit_finish();
        let Some(frame) = source
            .copy(answer.id(), no_finish, messages, options)
            .map_err(Error::Input)?
        else {
            return Ok(None);
        };
        Ok(Some(LocalResult {
            capacity: self.message_capabilities,
            frame,
            capabilities: Buffer::Inline(core::array::from_fn(|_| None)),
            count: 0,
            interface,
            method,
            prepared_return: Some((answer.id(), no_finish)),
        }))
    }

    /// Retain a hosted result envelope with a separately owned capability table.
    /// This is local answer storage, not publishable wire output: send_answer
    /// still stages grants and constructs the descriptor table transactionally.
    /// Every index is validated before retaining any supplied client, and an
    /// error returns all temporary frame and graph ownership.
    #[allow(clippy::too_many_arguments)]
    pub fn local_answer_prepared_with_capabilities<B: capnp::message::Allocator>(
        &mut self,
        answer: answers::Key<'id>,
        interface: u64,
        method: u16,
        source: &PreparedReturn<B>,
        capabilities: &[Option<&clients::Client<'id>>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<LocalResult<'pool, 'id, CAPS>>, Error> {
        if capabilities.is_empty() {
            return self
                .local_answer_prepared(answer, interface, method, source, messages, options);
        }
        self.check_live()?;
        let _ = self
            .ledgers
            .answers
            .prepare_return(answer)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
        let retained = self
            .message_capabilities
            .buffer::<_, CAPS>(capabilities.len(), || None)
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        let count = u32::try_from(capabilities.len())
            .map_err(|_| Error::Input(super::super::Error::Capacity))?;
        let frame =
            match source.copy_with_capabilities(answer.id(), false, count, messages, options) {
                Ok(Some(frame)) => frame,
                // The original builder can retain unreachable words. Let the
                // ordinary codec compact them before applying the frame-size limit.
                Ok(None) | Err(super::super::Error::Wire(capnp::ErrorKind::MessageTooLarge(_))) => {
                    return Ok(None)
                }
                Err(error) => return Err(Error::Input(error)),
            };
        let mut result = LocalResult {
            capacity: self.message_capabilities,
            frame,
            capabilities: retained,
            count: capabilities.len(),
            interface,
            method,
            prepared_return: None,
        };
        for (index, client) in capabilities.iter().enumerate() {
            let Some(client) = client else { continue };
            match self.runtime.retain(client) {
                Ok(client) => result.capabilities[index] = Some(client),
                Err(error) => {
                    result.release(messages, &mut self.runtime);
                    return Err(Error::Graph(error));
                }
            }
        }
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::{word, Word};
    use capntproto_rpc_engine::ids::WireId;
    #[test]
    fn retained_return_parts_preserve_source_limits_and_copy_fallback() {
        use capnp::message::{HeapAllocator, ReaderSegments};
        use capntproto_rpc_engine::connection::Growth;
        let mut source = PreparedReturn::from_template(
            &PreparedReturnTemplate::default(),
            HeapAllocator::new().first_segment_words(16),
        );
        source
            .results()
            .set_as::<capnp::data::Owned>(&[0xa5; 8192][..])
            .unwrap();
        let source = alloc::rc::Rc::new(source);
        let original = capnp::serialize::write_message_to_words(source.message.message());
        let mut pool = messages::GrowingBuffers::new(4, 16384, 65536);
        messages::with_growing_messages(
            &mut pool,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                for (id, no_finish) in [(0, false), (u32::MAX, true)] {
                    let parts = source
                        .retain_parts(
                            AnswerId::from_wire(id),
                            no_finish,
                            messages,
                            Default::default(),
                        )
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        parts.part(2).as_ptr(),
                        source.message.message().get_segment(1).unwrap().as_ptr()
                    );
                    let copied = source
                        .copy(
                            AnswerId::from_wire(id),
                            no_finish,
                            messages,
                            Default::default(),
                        )
                        .unwrap()
                        .unwrap();
                    let actual: alloc::vec::Vec<_> = (0..parts.count())
                        .flat_map(|i| parts.part(i).iter().copied())
                        .collect();
                    assert_eq!(&actual, &*messages.read(&copied.lease));
                    copied.release(messages);
                    drop(parts);
                    assert_eq!(
                        capnp::serialize::write_message_to_words(source.message.message()),
                        original
                    );
                }
                for strict in [
                    capnp::message::ReaderOptions {
                        nesting_limit: 0,
                        ..Default::default()
                    },
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: Some(1),
                        ..Default::default()
                    },
                ] {
                    assert!(source
                        .retain_parts(AnswerId::from_wire(1), true, messages, strict)
                        .is_err());
                }
            },
        )
        .unwrap();
        // Original allocator capacity may exceed a budget which can still hold
        // the compact copied frame. A failed retention must leave that fallback.
        let mut small = messages::GrowingBuffers::new(1, 16384, original.len() + 7);
        messages::with_growing_messages(
            &mut small,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                assert!(source
                    .retain_parts(AnswerId::from_wire(1), true, messages, Default::default())
                    .unwrap()
                    .is_none());
                source
                    .copy(AnswerId::from_wire(1), true, messages, Default::default())
                    .unwrap()
                    .unwrap()
                    .release(messages);
            },
        )
        .unwrap();
        let mut words = capnp::Word::allocate_zeroed_vec(original.len() / 8);
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            assert!(source
                .retain_parts(AnswerId::from_wire(1), true, messages, Default::default())
                .unwrap()
                .is_none());
            source
                .copy(AnswerId::from_wire(1), true, messages, Default::default())
                .unwrap()
                .unwrap()
                .release(messages);
        })
        .unwrap();
    }
    #[test]
    fn template_copies_preserve_independent_headers_payloads_and_limits() {
        let template = PreparedReturnTemplate::default();
        let mut first =
            PreparedReturn::from_template(&template, capnp::message::HeapAllocator::new());
        let mut second =
            PreparedReturn::from_template(&template, capnp::message::HeapAllocator::new());
        first
            .results()
            .set_as::<capnp::data::Owned>(b"first".as_slice())
            .unwrap();
        second
            .results()
            .set_as::<capnp::data::Owned>(b"second".as_slice())
            .unwrap();
        drop(template);
        let mut a = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let mut b = a;
        let mut buffers = [
            messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
            messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        ];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            for options in [
                capnp::message::ReaderOptions {
                    nesting_limit: 3,
                    ..Default::default()
                },
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(1),
                    ..Default::default()
                },
            ] {
                assert!(first
                    .copy(AnswerId::from_wire(0), false, messages, options)
                    .is_err());
            }
            let first_frame = first
                .copy(
                    AnswerId::from_wire(u32::MAX),
                    true,
                    messages,
                    Default::default(),
                )
                .unwrap()
                .unwrap();
            // Earlier validation failures consumed neither buffer. Header edits
            // affect this copy alone, even while both frames remain retained.
            let second_frame = second
                .copy(AnswerId::from_wire(0), false, messages, Default::default())
                .unwrap()
                .unwrap();
            first
                .results()
                .set_as::<capnp::data::Owned>(b"replaced".as_slice())
                .unwrap();
            for (frame, id, no_finish, expected) in [
                (&first_frame, u32::MAX, true, b"first".as_slice()),
                (&second_frame, 0, false, b"second".as_slice()),
            ] {
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Return(result) = root.which().unwrap() else {
                            panic!()
                        };
                        let result = result.unwrap();
                        assert_eq!(result.get_answer_id(), id);
                        assert_eq!(result.get_no_finish_needed(), no_finish);
                        assert!(!result.get_release_param_caps());
                        let rpc_capnp::return_::Results(payload) = result.which().unwrap() else {
                            panic!()
                        };
                        let payload = payload.unwrap();
                        assert!(payload.get_cap_table().unwrap().is_empty());
                        assert_eq!(
                            payload
                                .get_content()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            expected
                        );
                    })
                    .unwrap();
            }
            first_frame.release(messages);
            second_frame.release(messages);
        })
        .unwrap();
    }
    #[test]
    fn two_segment_returns_overwrite_reused_storage_and_preserve_far_data() {
        let payload: alloc::vec::Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
        let mut source = PreparedReturn::from_template(
            &PreparedReturnTemplate::default(),
            capnp::message::HeapAllocator::new().first_segment_words(16),
        );
        source
            .results()
            .set_as::<capnp::data::Owned>(payload.as_slice())
            .unwrap();
        assert_eq!(source.message.message().get_segments_for_output().len(), 2);
        let mut words = [word(0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa); 1024];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            for (id, no_finish) in [(u32::MAX, true), (0, false)] {
                let frame = source
                    .copy(
                        AnswerId::from_wire(id),
                        no_finish,
                        messages,
                        Default::default(),
                    )
                    .unwrap()
                    .unwrap();
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("Return")
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), id);
                        assert_eq!(returned.get_no_finish_needed(), no_finish);
                        assert!(!returned.get_release_param_caps());
                        let rpc_capnp::return_::Results(results) = returned.which().unwrap() else {
                            panic!("results")
                        };
                        assert_eq!(
                            results
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            payload
                        );
                    })
                    .unwrap();
                frame.release(messages);
            }
        })
        .unwrap();
        assert_eq!(
            &Word::words_to_bytes(&words)[12..16],
            &[0; 4],
            "table padding cannot expose old bytes"
        );
        assert_eq!(
            source
                .reader()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            payload
        );
    }
    #[test]
    fn segmented_returns_preserve_far_pointers_and_respect_the_reader_segment_limit() {
        for count in [1, 2, 509, 510] {
            let mut source = PreparedReturn::from_template(
                &PreparedReturnTemplate::default(),
                capnp::message::HeapAllocator::new()
                    .first_segment_words(8)
                    .allocation_strategy(capnp::message::AllocationStrategy::FixedSize),
            );
            let mut data = source
                .results()
                .initn_as::<capnp::data_list::Builder>(count);
            for index in 0..count {
                data.set(index, &[index as u8; 64]);
            }
            let segments = source.message.message().get_segments_for_output().len();
            assert_eq!(segments, count as usize + 2);
            let mut words = alloc::vec![word(0, 0, 0, 0, 0, 0, 0, 0); 10_000];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut words))];
            messages::with_messages(&mut buffers, &mut [], |messages| {
                let frame = source
                    .copy(AnswerId::from_wire(7), true, messages, Default::default())
                    .unwrap();
                if segments >= capnp::serialize::SEGMENTS_COUNT_LIMIT {
                    assert!(frame.is_none());
                    // Compaction is delegated before acquiring pool storage.
                    let writer = messages.reserve_exact(1).unwrap();
                    messages.discard(writer);
                    return;
                }
                let frame = frame.unwrap();
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!()
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), 7);
                        assert!(returned.get_no_finish_needed());
                        assert!(!returned.get_release_param_caps());
                        let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
                            panic!()
                        };
                        let data = payload
                            .unwrap()
                            .get_content()
                            .get_as::<capnp::data_list::Reader>()
                            .unwrap();
                        assert_eq!(data.len(), count);
                        for index in 0..count {
                            assert_eq!(data.get(index).unwrap(), &[index as u8; 64]);
                        }
                    })
                    .unwrap();
                frame.release(messages);
            })
            .unwrap();
        }
    }
}
