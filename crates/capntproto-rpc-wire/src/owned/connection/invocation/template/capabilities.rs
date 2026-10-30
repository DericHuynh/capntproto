//! Append descriptors to certified private Call layouts without walking content.
use super::*;
use crate::owned::connection::payload::{descriptor_bytes, PayloadSegments};
use crate::owned::Error as EncodeError;

// A private immutable payload and its separately published descriptor pointer.
// Appending descriptors preserves every original pointer and capability index.
pub(in crate::owned::connection::invocation) struct EncodedCapabilities<'a> {
    pub(super) call: EncodedCall<'a>,
    pub(super) table: usize,
}
impl EncodedCapabilities<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::owned::connection::invocation) fn copy<'pool>(
        &self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        question: QuestionId,
        target: ImportId,
        method: calls::DynamicCall,
        count: usize,
        minimum: usize,
        extra: usize,
        allow_handoff: bool,
        options: capnp::message::ReaderOptions,
        fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
    ) -> Result<Option<Frame<'pool>>, EncodeError> {
        let template = self
            .call
            .for_publication(method, options, WriterPolicy::Native)?;
        let first = self.call.segment;
        let second = self.call.second.unwrap_or(&[]);
        let header_len = if self.call.second.is_some() { 16 } else { 8 };
        let descriptor_bytes = descriptor_bytes(count, extra)?;
        let source_end = first
            .len()
            .checked_add(second.len())
            .and_then(|n| n.checked_add(header_len))
            .ok_or(EncodeError::Capacity)?;
        let capacity = source_end
            .checked_add(descriptor_bytes)
            .ok_or(EncodeError::Capacity)?
            .max(minimum);
        let writer = match messages.reserve(capacity) {
            Ok(writer) => writer,
            Err(messages::Error::NoBuffer) => return Ok(None),
            Err(error) => return Err(EncodeError::Storage(error)),
        };
        let result = (|| {
            let mut view = messages.try_write(&writer).map_err(EncodeError::Storage)?;
            self.write(
                &mut view,
                template,
                question,
                target,
                count,
                descriptor_bytes,
                allow_handoff,
                fill,
            )
        })();
        let length = match result {
            Ok(length) => length,
            Err(error) => {
                messages.discard(writer);
                return Err(error);
            }
        };
        match messages.seal(writer, length) {
            Ok(lease) => Ok(Some(Frame::new(lease, options))),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(EncodeError::Storage(rejected.error))
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn write(
        &self,
        view: &mut [u8],
        template: PlainTemplate,
        question: QuestionId,
        target: ImportId,
        count: usize,
        descriptor_bytes: usize,
        allow_handoff: bool,
        fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
    ) -> Result<usize, EncodeError> {
        let payload = PayloadSegments {
            first: self.call.segment,
            table: self.table,
        };
        write_call(
            payload,
            self.call.second.into_iter(),
            view,
            template,
            question,
            target,
            count,
            descriptor_bytes,
            allow_handoff,
            fill,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn write_call<'a>(
    payload: PayloadSegments<'a>,
    tail: impl ExactSizeIterator<Item = &'a [u8]>,
    view: &mut [u8],
    template: PlainTemplate,
    question: QuestionId,
    target: ImportId,
    count: usize,
    descriptor_bytes: usize,
    allow_handoff: bool,
    fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
) -> Result<usize, EncodeError> {
    let header = (tail.len().div_ceil(2) + 1) * 8;
    let written = payload.write(view, tail, count, descriptor_bytes, fill)?;
    let body = &mut view[header..written.first_end];
    body[template.call..template.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
    body[template.target..template.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
    body[template.call + 16] = (body[template.call + 16] & !0b111)
        | u8::from(allow_handoff)
        | (u8::from(template.no_promise_pipelining) << 1);
    Ok(written.length)
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use capnp::{word, Word};

    #[test]
    fn generic_segmented_calls_append_descriptors_without_compacting_their_payload() {
        use capnp::message::{AllocationStrategy, Builder, HeapAllocator, ReaderSegments};
        let mut call = PreparedCall::new(
            Builder::new(
                HeapAllocator::new()
                    .first_segment_words(32)
                    .allocation_strategy(AllocationStrategy::FixedSize),
            ),
            7,
            9,
        )
        .unwrap();
        for _ in 0..2 {
            call.params()
                .initn_as::<capnp::data::Builder>(320)
                .fill(0xaa);
        }
        let data = [0x5a; 640];
        call.params()
            .set_as::<capnp::data::Owned>(&data[..])
            .unwrap();
        assert_eq!(call.message().len(), 4);
        let bytes = capnp::serialize::write_message_to_words(call.message());
        let mut a = [word(0, 0, 0, 0, 0, 0, 0, 0); 512];
        let mut b = a;
        let mut buffers = [
            messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
            messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        ];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let source = Frame::copy_from(messages, &bytes, Default::default()).unwrap();
            let mut copied = None;
            let measured = allocation_counter::measure(|| {
                copied = copy_frame(
                    &source,
                    None,
                    messages,
                    QuestionId::from_wire(u32::MAX),
                    ImportId::from_wire(42),
                    3,
                    128,
                    0,
                    true,
                    Default::default(),
                    |mut table| {
                        table
                            .get_mut(0)
                            .unwrap()
                            .scalar(CapDescriptor::SENDER_HOSTED)
                            .set(11);
                        table.get_mut(1).unwrap().void(CapDescriptor::NONE).set();
                        table
                            .get_mut(2)
                            .unwrap()
                            .scalar(CapDescriptor::RECEIVER_HOSTED)
                            .set(13);
                        Ok(())
                    },
                )
                .unwrap();
            });
            assert_eq!(measured.count_total, 0);
            let (copied, method) = copied.unwrap();
            assert_eq!(method, (7, 9));
            {
                let input = messages.read(&copied.lease);
                let actual = copied.reader(&input).unwrap();
                assert_eq!(actual.get_segments().len(), call.message().len());
                for index in 1..call.message().len() {
                    assert_eq!(
                        actual.get_segments().get_segment(index as u32),
                        call.message().get_segment(index as u32)
                    );
                }
            }
            copied
                .with(messages, |root| {
                    let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                        panic!("Call")
                    };
                    let call = call.unwrap();
                    assert_eq!(call.get_question_id(), u32::MAX);
                    assert!(call.get_allow_third_party_tail_call());
                    assert!(matches!(
                        call.get_target().unwrap().which().unwrap(),
                        rpc_capnp::message_target::ImportedCap(42)
                    ));
                    let payload = call.get_params().unwrap();
                    assert_eq!(
                        payload
                            .get_content()
                            .get_as::<capnp::data::Reader>()
                            .unwrap(),
                        data
                    );
                    let table = payload.get_cap_table().unwrap();
                    assert_eq!(table.len(), 3);
                    assert!(matches!(
                        table.get(0).which().unwrap(),
                        rpc_capnp::cap_descriptor::SenderHosted(11)
                    ));
                    assert!(matches!(
                        table.get(1).which().unwrap(),
                        rpc_capnp::cap_descriptor::None(())
                    ));
                    assert!(matches!(
                        table.get(2).which().unwrap(),
                        rpc_capnp::cap_descriptor::ReceiverHosted(13)
                    ));
                })
                .unwrap();
            copied.release(messages);
            source.release(messages);
        })
        .unwrap();
    }

    #[test]
    fn retained_header_publishes_without_reopening_its_reader_budget() {
        for length in [0, 64, 320] {
            let mut call = PreparedCall::new(
                capnp::message::Builder::new(
                    capnp::message::HeapAllocator::new().first_segment_words(32),
                ),
                7,
                9,
            )
            .unwrap();
            let data = alloc::vec![0x5a; length];
            call.params()
                .set_as::<capnp::data::Owned>(&data[..])
                .unwrap();
            let method = call.method;
            let encoded = call.encoded_for_sending(method).unwrap().unwrap();
            let template = encoded
                .for_publication(method, Default::default(), WriterPolicy::Native)
                .unwrap();
            let mut a = [word(0, 0, 0, 0, 0, 0, 0, 0); 256];
            let mut b = a;
            let mut buffers = [
                messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
                messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
            ];
            messages::with_messages(&mut buffers, &mut [], |messages| {
                let mut source = encoded
                    .copy(
                        template,
                        messages,
                        QuestionId::from_wire(0),
                        ImportId::from_wire(0),
                        false,
                        Default::default(),
                    )
                    .unwrap();
                let prepared =
                    encoded.framed_template(template, messages.read(&source.lease).len());
                // Instrument another envelope read as a failure. The known
                // writer path must use its retained geometry instead.
                source.budget = capnp::message::ReadBudget::new(Some(0));
                let fill = |mut table: capnp::field_api::FixedList<'_, CapDescriptor>| {
                    table
                        .get_mut(0)
                        .unwrap()
                        .scalar(CapDescriptor::SENDER_HOSTED)
                        .set(11);
                    table.get_mut(1).unwrap().void(CapDescriptor::NONE).set();
                    table
                        .get_mut(2)
                        .unwrap()
                        .scalar(CapDescriptor::RECEIVER_HOSTED)
                        .set(13);
                    Ok(())
                };
                assert!(matches!(
                    copy_frame(
                        &source,
                        None,
                        messages,
                        QuestionId::from_wire(37),
                        ImportId::from_wire(42),
                        3,
                        128,
                        0,
                        true,
                        Default::default(),
                        fill
                    ),
                    Err(EncodeError::Wire(capnp::ErrorKind::ReadLimitExceeded))
                ));
                let mut copied = None;
                let measured = allocation_counter::measure(|| {
                    copied = copy_frame(
                        &source,
                        Some(prepared),
                        messages,
                        QuestionId::from_wire(37),
                        ImportId::from_wire(42),
                        3,
                        128,
                        0,
                        true,
                        Default::default(),
                        fill,
                    )
                    .unwrap();
                });
                assert_eq!(measured.count_total, 0);
                let (copied, method) = copied.unwrap();
                assert_eq!(method, (7, 9));
                copied
                    .with(messages, |root| {
                        let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                            panic!("Call");
                        };
                        let call = call.unwrap();
                        assert_eq!(call.get_question_id(), 37);
                        assert_eq!((call.get_interface_id(), call.get_method_id()), (7, 9));
                        assert!(matches!(
                            call.get_target().unwrap().which().unwrap(),
                            rpc_capnp::message_target::ImportedCap(42)
                        ));
                        assert!(call.get_no_promise_pipelining());
                        assert!(call.get_allow_third_party_tail_call());
                        let payload = call.get_params().unwrap();
                        assert_eq!(
                            payload
                                .get_content()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            data
                        );
                        let table = payload.get_cap_table().unwrap();
                        assert_eq!(table.len(), 3);
                        assert!(matches!(
                            table.get(0).which().unwrap(),
                            rpc_capnp::cap_descriptor::SenderHosted(11)
                        ));
                        assert!(matches!(
                            table.get(1).which().unwrap(),
                            rpc_capnp::cap_descriptor::None(())
                        ));
                        assert!(matches!(
                            table.get(2).which().unwrap(),
                            rpc_capnp::cap_descriptor::ReceiverHosted(13)
                        ));
                    })
                    .unwrap();
                copied.release(messages);
                source.release(messages);
            })
            .unwrap();
        }
    }
}

/// The invocation owns a safely built private Call and a separate client table.
/// Keep all of its payload pointers when appending the final wire descriptors.
type CopiedCall<'pool> = (Frame<'pool>, (u64, u16));

#[allow(clippy::too_many_arguments)]
pub(in crate::owned::connection::invocation) fn copy_frame<'pool>(
    source: &Frame<'pool>,
    prepared: Option<PlainTemplate>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    question: QuestionId,
    target: ImportId,
    count: usize,
    minimum: usize,
    extra: usize,
    allow_handoff: bool,
    options: capnp::message::ReaderOptions,
    fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
) -> Result<Option<CopiedCall<'pool>>, EncodeError> {
    use capnp::message::ReaderSegments;
    let descriptor_bytes = descriptor_bytes(count, extra)?;
    let capacity = messages
        .read(&source.lease)
        .len()
        .checked_add(descriptor_bytes)
        .ok_or(EncodeError::Capacity)?
        .max(minimum);
    let mut writer = match messages.reserve(capacity) {
        Ok(writer) => writer,
        Err(messages::Error::NoBuffer) => return Ok(None),
        Err(error) => return Err(EncodeError::Storage(error)),
    };
    let copy = |root: Option<rpc_capnp::message::Reader<'_>>| -> Result<_, EncodeError> {
        let input = messages.read(&source.lease);
        let reader = source.reader(&input)?;
        let segments = reader.get_segments();
        let first = segments.get_segment(0).ok_or(EncodeError::WrongMessage)?;
        let (template, table) = if let Some(prepared) = prepared {
            // Only a private prepared Call can retain capability-bearing
            // geometry. Copying its frame preserves every relative pointer.
            let header = (first.as_ptr() as usize)
                .checked_sub(input.as_ptr() as usize)
                .ok_or(EncodeError::WrongMessage)?;
            let template = prepared
                .in_segment(header, first.len())
                .ok_or(EncodeError::WrongMessage)?;
            let table = descriptor_pointer_offset(template.target);
            debug_assert!(table + 8 <= first.len());
            (template, table)
        } else {
            let root = root.expect("uncertified Call needs its reader");
            let rpc_capnp::message::Call(call) =
                root.which().map_err(|_| EncodeError::WrongMessage)?
            else {
                return Ok(None);
            };
            let call = call?;
            let Some(template) = PlainTemplate::new(call, first) else {
                return Ok(None);
            };
            let Some(payload) = PayloadSegments::new(first, call.get_params()?) else {
                return Ok(None);
            };
            (template, payload.table)
        };
        if template.only_promise_pipeline {
            return Ok(None);
        }
        let mut view = messages
            .write_borrowed(&mut writer)
            .map_err(EncodeError::Storage)?;
        let length = write_call(
            PayloadSegments { first, table },
            (1..segments.len())
                .map(|index| segments.get_segment(index as u32).expect("frame segment")),
            &mut view,
            template,
            question,
            target,
            count,
            descriptor_bytes,
            allow_handoff,
            fill,
        )?;
        Ok(Some((length, template.method)))
    };
    let result = if prepared.is_some() {
        copy(None)
    } else {
        // General frames retain the original reader and shared traversal budget.
        source
            .with_borrowed_root(messages, |root| copy(Some(root)))
            .and_then(core::convert::identity)
    };
    match result {
        Ok(Some((length, method))) => match messages.seal(writer, length) {
            Ok(lease) => Ok(Some((Frame::new(lease, options), method))),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(EncodeError::Storage(rejected.error))
            }
        },
        Ok(None) => {
            messages.discard(writer);
            Ok(None)
        }
        Err(error) => {
            messages.discard(writer);
            Err(error)
        }
    }
}
