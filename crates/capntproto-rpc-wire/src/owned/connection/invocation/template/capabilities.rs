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
    pub(super) fn new<'a>(
        call: EncodedCall<'a>,
        payload: rpc_capnp::payload::Reader<'_>,
    ) -> Result<Option<EncodedCapabilities<'a>>, EncodeError> {
        Ok(
            PayloadSegments::new(call.segment, call.second, payload).map(|payload| {
                EncodedCapabilities {
                    call,
                    table: payload.table,
                }
            }),
        )
    }
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
            second: self.call.second,
            table: self.table,
        };
        let written = payload.write(view, count, descriptor_bytes, fill)?;
        let header = if self.call.second.is_some() { 16 } else { 8 };
        let body = &mut view[header..written.first_end];
        body[template.call..template.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
        body[template.target..template.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
        body[template.call + 16] = (body[template.call + 16] & !0b111)
            | u8::from(allow_handoff)
            | (u8::from(template.no_promise_pipelining) << 1);
        Ok(written.length)
    }
}

/// The invocation owns a safely built private Call and a separate client table.
/// Keep all of its payload pointers when appending the final wire descriptors.
type CopiedCall<'pool> = (Frame<'pool>, (u64, u16));

#[allow(clippy::too_many_arguments)]
pub(in crate::owned::connection::invocation) fn copy_frame<'pool>(
    source: &Frame<'pool>,
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
    let result = source
        .with_borrowed_root(messages, |root| -> Result<_, EncodeError> {
            let rpc_capnp::message::Call(call) =
                root.which().map_err(|_| EncodeError::WrongMessage)?
            else {
                return Ok(None);
            };
            let call = call?;
            let input = messages.read(&source.lease);
            let reader = source.reader(&input)?;
            let segments = reader.get_segments();
            let second = match segments.len() {
                1 => None,
                2 => segments.get_segment(1),
                _ => return Ok(None),
            };
            let first = segments.get_segment(0).ok_or(EncodeError::WrongMessage)?;
            let Some(template) = PlainTemplate::new(call, first) else {
                return Ok(None);
            };
            if template.only_promise_pipeline {
                return Ok(None);
            }
            let encoded = EncodedCall {
                segment: first,
                second,
                template,
                options: None,
                #[cfg(feature = "alloc")]
                owner: None,
            };
            let Some(encoded) = EncodedCapabilities::new(encoded, call.get_params()?)? else {
                return Ok(None);
            };
            let mut view = messages
                .write_borrowed(&mut writer)
                .map_err(EncodeError::Storage)?;
            let length = encoded.write(
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
        })
        .and_then(core::convert::identity);
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
