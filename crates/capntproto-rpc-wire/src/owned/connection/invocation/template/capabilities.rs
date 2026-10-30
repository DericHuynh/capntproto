//! Append descriptors to certified private Call layouts without walking content.
use super::*;
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
        mut fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
    ) -> Result<Option<Frame<'pool>>, EncodeError> {
        let template = self
            .call
            .for_publication(method, options, WriterPolicy::Native)?;
        let first = self.call.segment;
        let second = self.call.second.unwrap_or(&[]);
        let header_len = if self.call.second.is_some() { 16 } else { 8 };
        let descriptor_bytes = count
            .checked_mul(
                usize::from(CapDescriptor::SIZE.data) + usize::from(CapDescriptor::SIZE.pointers),
            )
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(16))
            .and_then(|n| n.checked_add(extra))
            .ok_or(EncodeError::Capacity)?;
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
            let start = header_len + first.len();
            view[header_len..start].copy_from_slice(first);
            let (table_bytes, root) = {
                let mut builder = capnp::message::FixedBuilder::from_bytes(
                    &mut view[start..start + descriptor_bytes],
                )?;
                fill(builder.try_init_root_list::<CapDescriptor>(count)?)?;
                let segment = builder.into_segment();
                let root: [u8; 8] = segment[..8].try_into().expect("descriptor root");
                (segment.len(), root)
            };
            // Move the descriptor list after the original first segment. Its
            // temporary root is replaced by Payload.capTable's near pointer.
            // Existing segment IDs, relative offsets and far pointers remain
            // unchanged, including references into the large second segment.
            view.copy_within(start + 8..start + table_bytes, start);
            let first_end = start + table_bytes - 8;
            let length = first_end
                .checked_add(second.len())
                .ok_or(EncodeError::Capacity)?;
            view[first_end..length].copy_from_slice(second);
            view[..header_len].fill(0);
            let first_words =
                u32::try_from((first_end - header_len) / 8).map_err(|_| EncodeError::Capacity)?;
            view[4..8].copy_from_slice(&first_words.to_le_bytes());
            if self.call.second.is_some() {
                view[..4].copy_from_slice(&1u32.to_le_bytes());
                let second_words =
                    u32::try_from(second.len() / 8).map_err(|_| EncodeError::Capacity)?;
                view[8..12].copy_from_slice(&second_words.to_le_bytes());
            }
            self.patch(
                &mut view[header_len..first_end],
                root,
                template,
                question,
                target,
                allow_handoff,
            )?;
            Ok(length)
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
    fn patch(
        &self,
        body: &mut [u8],
        root: [u8; 8],
        template: PlainTemplate,
        question: QuestionId,
        target: ImportId,
        allow_handoff: bool,
    ) -> Result<(), EncodeError> {
        let root_low = u32::from_le_bytes(root[..4].try_into().unwrap());
        if root_low != 1 {
            return Err(EncodeError::WrongMessage);
        }
        let offset = (self.call.segment.len() - self.table - 8) / 8;
        if offset >= 1 << 29 {
            return Err(EncodeError::Capacity);
        }
        body[template.call..template.call + 4].copy_from_slice(&question.to_wire().to_le_bytes());
        body[template.target..template.target + 4].copy_from_slice(&target.to_wire().to_le_bytes());
        body[template.call + 16] = (body[template.call + 16] & !0b111)
            | u8::from(allow_handoff)
            | (u8::from(template.no_promise_pipelining) << 1);
        body[self.table..self.table + 4]
            .copy_from_slice(&(((offset as u32) << 2) | 1).to_le_bytes());
        body[self.table + 4..self.table + 8].copy_from_slice(&root[4..]);
        Ok(())
    }
}
