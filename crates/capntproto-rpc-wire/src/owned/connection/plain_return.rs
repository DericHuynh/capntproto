//! Fuse receipt-free reply admission and ownership handoff. General controls,
//! fixed buffers, cancellation and pipeline observers retain the ordinary path.
use super::*;

// Decline only the common single-segment envelope when it cannot be a plain
// reply. This is a storage hint, never an admission certificate: accepted and
// noncanonical frames still pass through the complete existing validation.
// Avoid copying a callback or a capability-bearing parent Return into a shared
// reply owner only to discard it and copy it again for ordinary admission.
#[cfg(feature = "alloc")]
#[inline]
fn canonical_nonplain(bytes: &[u8]) -> bool {
    let Some(prefix) = bytes.get(..56) else {
        return false;
    };
    if prefix[..4] != [0; 4]
        || (u32::from_le_bytes(prefix[4..8].try_into().unwrap()) as usize)
            .checked_mul(8)
            != Some(bytes.len() - 8)
        // Root at segment word zero, followed by Message's one data word and
        // one pointer. Unknown layouts retain the general decoding path.
        || prefix[8..16] != 0x0001_0001_0000_0000u64.to_le_bytes()
    {
        return false;
    }
    // Message.return has tag 3. Return's canonical body has two data words and
    // one pointer; noFinishNeeded is bit 33 of its data section (default false).
    prefix[16..18] != 3u16.to_le_bytes()
        || (prefix[24..32] == 0x0001_0002_0000_0000u64.to_le_bytes() && prefix[36] & 2 == 0)
}

#[cfg(feature = "alloc")]
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Try a complete owned data-only Return with no parameter-grant releases.
    /// The selector runs before admission and must identify its direct sink;
    /// returning None preserves the entire frame and question for normal receive.
    /// Selection is tentative: malformed or ineligible replies can still defer
    /// or fail, so the selector must retain its sink until Delivered is returned.
    /// No reference, pipeline, grant, authentication or ordering fence is bypassed.
    /// A successful result needs no subsequent take_output/release_response.
    #[allow(clippy::result_large_err)]
    #[inline(never)]
    pub fn try_receive_plain_owned<T>(
        &mut self,
        frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        select: impl FnOnce(questions::Key<'id>) -> Option<T>,
    ) -> Result<PlainReceived<'pool, T>, (Error, Frame<'pool>)> {
        if self.disconnected.is_some()
            || self.resolution.is_some()
            || self.ledgers.questions.is_empty()
            || self.has_ordered_propagation()
        {
            return Ok(PlainReceived::Deferred(frame));
        }
        let source = match frame.shared_reader(messages) {
            Ok(Some(reader)) => reader,
            Ok(None) => return Ok(PlainReceived::Deferred(frame)),
            Err(error) => return Err((Error::Input(error), frame)),
        };
        match self.receive_plain_reader(source, messages, select) {
            Ok(Some((receiver, message))) => {
                frame.release(messages);
                Ok(PlainReceived::Delivered(receiver, message))
            }
            Ok(None) => Ok(PlainReceived::Deferred(frame)),
            Err(error) => Err((error, frame)),
        }
    }

    /// Admit a complete buffered reply directly into a bounded shared owner.
    /// None has no protocol effects and leaves the caller's input unconsumed.
    /// Fixed storage and ineligible replies use the ordinary frame path.
    #[inline(always)]
    pub fn try_receive_plain_buffered<T>(
        &mut self,
        bytes: &[u8],
        options: capnp::message::ReaderOptions,
        messages: &mut messages::Messages<'pool, '_, '_>,
        select: impl FnOnce(questions::Key<'id>) -> Option<T>,
    ) -> Result<Option<(T, alloc::boxed::Box<super::super::PlainResults>)>, Error> {
        if self.disconnected.is_some()
            || self.resolution.is_some()
            || self.ledgers.questions.is_empty()
            || self.has_ordered_propagation()
        {
            return Ok(None);
        }
        if canonical_nonplain(bytes) {
            return Ok(None);
        }
        let Some(bytes) = messages
            .copy_shared(bytes)
            .map_err(|error| Error::Input(super::super::Error::Storage(error)))?
        else {
            return Ok(None);
        };
        let segments = capnp::serialize::NoAllocBufferSegments::from_buffer_exact(bytes, options)
            .map_err(|error| Error::Input(error.into()))?;
        let source = capnp::message::Reader::new(segments, options)
            .into_shared()
            .map_err(|error| Error::Input(error.into()))?;
        self.receive_plain_reader(&source, messages, select)
    }

    // Keep reply validation and ownership handoff out of each receive-pump
    // expansion. Buffered and retained-frame inputs share this transition.
    #[inline(never)]
    fn receive_plain_reader<T>(
        &mut self,
        source: &capnp::message::SharedReader<
            capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
        >,
        messages: &mut messages::Messages<'pool, '_, '_>,
        select: impl FnOnce(questions::Key<'id>) -> Option<T>,
    ) -> Result<Option<(T, alloc::boxed::Box<super::super::PlainResults>)>, Error> {
        let admitted = (|| -> Result<_, super::super::Error> {
            let root = source.get_root::<capnp::any_pointer::Reader>()?;
            let root = root.get_as::<rpc_capnp::message::Reader>()?;
            use capnp::traits::IntoInternalStructReader;
            let fields = rpc_capnp::api::MessageRef::from(root.into_internal_struct_reader());
            if fields.tag() != rpc_capnp::api::MessageUnionTag::Return {
                return Ok(None);
            }
            let result =
                rpc_capnp::return_::Reader::from(fields.r#return()?.into_internal_struct_reader());
            if !result.get_no_finish_needed() {
                return Ok(None);
            }
            let id = result.get_answer_id();
            let Some(index) =
                ExchangeSlot::pending_index(&self.exchanges, &self.ledgers.questions, id)
            else {
                return Ok(None);
            };
            let slot = &self.exchanges[index];
            if !slot.authorized || slot.canceled {
                return Ok(None);
            }
            let Some(Exchange::Pending(question)) = &slot.exchange else {
                unreachable!("selected pending question")
            };
            let key = question.key();
            if self.outgoing.pending_for(key)
                || self
                    .invocation_pipelines
                    .iter()
                    .any(|slot| slot.pending_for(key))
            {
                return Ok(None);
            }
            let Some(receiver) = select(key) else {
                return Ok(None);
            };
            let Some((pending, content)) =
                question.prepare_plain_return(result, &mut self.ledgers.questions, true)?
            else {
                return Ok(None);
            };
            let message = source.retain(content)?;
            Ok(Some((message, (index, receiver, pending))))
        })();
        match admitted {
            Ok(Some((message, (index, receiver, pending)))) => {
                // Retention completed before committing the question transition.
                let admitted = super::super::question::AdmittedPlain::owned(message);
                let Some(Exchange::Pending(question)) = self.exchanges[index].exchange.take()
                else {
                    unreachable!("selected original reply owner")
                };
                let message = question.finish_plain(messages, pending, admitted);
                Ok(Some((receiver, message)))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(Error::Input(error)),
        }
    }
}
