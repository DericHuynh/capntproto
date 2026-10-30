//! Append a separately staged descriptor table to immutable local payload bytes.
use super::*;
use crate::owned::Error;
use crate::rpc_capnp::api::CapDescriptor;
use capnp::{field_api::Schema, traits::IntoInternalStructReader};

pub(super) struct PayloadSegments<'a> {
    pub first: &'a [u8],
    pub second: Option<&'a [u8]>,
    pub table: usize,
}
pub(super) struct WrittenPayload {
    pub length: usize,
    pub first_end: usize,
}
impl<'a> PayloadSegments<'a> {
    pub fn new(
        first: &'a [u8],
        second: Option<&'a [u8]>,
        payload: rpc_capnp::payload::Reader<'_>,
    ) -> Option<Self> {
        let pointers = payload
            .into_internal_struct_reader()
            .get_pointer_section_as_blob();
        let table = (pointers.as_ptr() as usize)
            .checked_add(8)?
            .checked_sub(first.as_ptr() as usize)?;
        if pointers.len() < 16 || table.checked_add(8)? > first.len() {
            return None;
        }
        Some(Self {
            first,
            second,
            table,
        })
    }
    pub fn write(
        &self,
        view: &mut [u8],
        count: usize,
        descriptor_bytes: usize,
        mut fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
    ) -> Result<WrittenPayload, Error> {
        let header = if self.second.is_some() { 16 } else { 8 };
        let start = header + self.first.len();
        view[header..start].copy_from_slice(self.first);
        let (table_bytes, root) = {
            let mut builder = capnp::message::FixedBuilder::from_bytes(
                &mut view[start..start + descriptor_bytes],
            )?;
            fill(builder.try_init_root_list::<CapDescriptor>(count)?)?;
            let segment = builder.into_segment();
            let root: [u8; 8] = segment[..8].try_into().expect("descriptor root");
            (segment.len(), root)
        };
        // Moving every descriptor and its metadata by the same word preserves
        // their relative pointers. Existing payload segment IDs never change.
        view.copy_within(start + 8..start + table_bytes, start);
        let first_end = start + table_bytes - 8;
        let second = self.second.unwrap_or(&[]);
        let length = first_end.checked_add(second.len()).ok_or(Error::Capacity)?;
        view[first_end..length].copy_from_slice(second);
        view[..header].fill(0);
        let first_words = u32::try_from((first_end - header) / 8).map_err(|_| Error::Capacity)?;
        view[4..8].copy_from_slice(&first_words.to_le_bytes());
        if self.second.is_some() {
            view[..4].copy_from_slice(&1u32.to_le_bytes());
            let second_words = u32::try_from(second.len() / 8).map_err(|_| Error::Capacity)?;
            view[8..12].copy_from_slice(&second_words.to_le_bytes());
        }
        if u32::from_le_bytes(root[..4].try_into().unwrap()) != 1 {
            return Err(Error::WrongMessage);
        }
        let offset = (self.first.len() - self.table - 8) / 8;
        if offset >= 1 << 29 {
            return Err(Error::Capacity);
        }
        let body = &mut view[header..first_end];
        body[self.table..self.table + 4]
            .copy_from_slice(&(((offset as u32) << 2) | 1).to_le_bytes());
        body[self.table + 4..self.table + 8].copy_from_slice(&root[4..]);
        Ok(WrittenPayload { length, first_end })
    }
}
pub(super) fn descriptor_bytes(count: usize, extra: usize) -> Result<usize, Error> {
    count
        .checked_mul(
            usize::from(CapDescriptor::SIZE.data) + usize::from(CapDescriptor::SIZE.pointers),
        )
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| n.checked_add(16))
        .and_then(|n| n.checked_add(extra))
        .ok_or(Error::Capacity)
}

/// Local results are privately built and retain their clients separately. This
/// only constructs wire bytes; answer/grant admission remains the caller's job.
#[allow(clippy::too_many_arguments)]
pub(super) fn copy_return_frame<'pool>(
    source: &Frame<'pool>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    answer: capntproto_rpc_engine::ids::AnswerId,
    count: usize,
    minimum: usize,
    extra: usize,
    options: capnp::message::ReaderOptions,
    fill: impl FnMut(capnp::field_api::FixedList<'_, CapDescriptor>) -> capnp::Result<()>,
) -> Result<Option<Frame<'pool>>, Error> {
    use capnp::message::ReaderSegments;
    let descriptor_bytes = descriptor_bytes(count, extra)?;
    let capacity = messages
        .read(&source.lease)
        .len()
        .checked_add(descriptor_bytes)
        .ok_or(Error::Capacity)?
        .max(minimum);
    let mut writer = match messages.reserve(capacity) {
        Ok(writer) => writer,
        Err(messages::Error::NoBuffer) => return Ok(None),
        Err(error) => return Err(Error::Storage(error)),
    };
    let result = source
        .with_borrowed_root(messages, |root| -> Result<_, Error> {
            let rpc_capnp::message::Return(returned) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Ok(None);
            };
            let returned = returned?;
            if returned.get_release_param_caps() {
                return Ok(None);
            }
            let rpc_capnp::return_::Results(payload) =
                returned.which().map_err(|_| Error::WrongMessage)?
            else {
                return Ok(None);
            };
            let payload = payload?;
            if !payload.get_cap_table()?.is_empty() {
                return Ok(None);
            }
            let input = messages.read(&source.lease);
            let reader = source.reader(&input)?;
            let segments = reader.get_segments();
            let second = match segments.len() {
                1 => None,
                2 => segments.get_segment(1),
                _ => return Ok(None),
            };
            let first = segments.get_segment(0).ok_or(Error::WrongMessage)?;
            let data = returned
                .into_internal_struct_reader()
                .get_data_section_as_blob();
            let Some(offset) = (data.as_ptr() as usize)
                .checked_sub(first.as_ptr() as usize)
                .filter(|offset| {
                    data.len() >= 5
                        && offset
                            .checked_add(data.len())
                            .is_some_and(|end| end <= first.len())
                })
            else {
                return Ok(None);
            };
            let Some(payload) = PayloadSegments::new(first, second, payload) else {
                return Ok(None);
            };
            let mut view = messages
                .write_borrowed(&mut writer)
                .map_err(Error::Storage)?;
            let written = payload.write(&mut view, count, descriptor_bytes, fill)?;
            let header = if second.is_some() { 16 } else { 8 };
            let body = &mut view[header..written.first_end];
            body[offset..offset + 4].copy_from_slice(&answer.to_wire().to_le_bytes());
            // Capability-bearing replies always require the peer's Finish.
            body[offset + 4] &= !2;
            Ok(Some(written.length))
        })
        .and_then(core::convert::identity);
    match result {
        Ok(Some(length)) => match messages.seal(writer, length) {
            Ok(lease) => Ok(Some(Frame::new(lease, options))),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(Error::Storage(rejected.error))
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
