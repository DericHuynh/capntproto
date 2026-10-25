//! Fuse receipt-free reply admission and ownership handoff. General controls,
//! fixed buffers, cancellation and pipeline observers retain the ordinary path.
use super::*;

#[cfg(feature = "alloc")]
enum PlainSource<'a> {
    Unique(
        capnp::message::Reader<capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>>,
        Option<&'a core::cell::Cell<Option<usize>>>,
    ),
    Shared(
        &'a capnp::message::SharedReader<
            capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
        >,
    ),
}
#[cfg(feature = "alloc")]
impl PlainSource<'_> {
    fn select<T>(
        self,
        select: impl for<'a> FnOnce(
            capnp::any_pointer::Reader<'a>,
        ) -> Result<
            Option<(capnp::any_pointer::Reader<'a>, T)>,
            super::super::Error,
        >,
    ) -> Result<Option<(super::super::question::PlainReader, T)>, super::super::Error> {
        use super::super::question::PlainReader;
        match self {
            Self::Unique(source, remaining) => source
                .try_select_pointer_or_else(select, |reader| {
                    if let Some(remaining) = remaining {
                        remaining.set(reader.read_budget().remaining_words());
                    }
                })
                .map(|result| result.map(|(message, value)| (PlainReader::Unique(message), value))),
            Self::Shared(source) => {
                let Some((pointer, value)) = select(source.get_root()?)? else {
                    return Ok(None);
                };
                Ok(Some((PlainReader::Shared(source.retain(pointer)?), value)))
            }
        }
    }
}

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
        mut frame: Frame<'pool>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        select: impl FnOnce(questions::Key<'id>, Option<usize>) -> Option<T>,
    ) -> Result<PlainReceived<'pool, T>, (Error, Frame<'pool>)> {
        if self.disconnected.is_some()
            || self.resolution.is_some()
            || self.ledgers.questions.is_empty()
            || self.has_ordered_propagation()
        {
            return Ok(PlainReceived::Deferred(frame));
        }
        let remaining = core::cell::Cell::new(frame.budget.remaining_words());
        let outcome = if let Some(source) = frame.reader.get() {
            // Existing observers must continue sharing the exact same budget.
            self.receive_plain_reader(PlainSource::Shared(source), messages, select)
        } else {
            let segments = match frame.share_segments(messages) {
                Ok(Some(segments)) => segments,
                Ok(None) => return Ok(PlainReceived::Deferred(frame)),
                Err(error) => return Err((Error::Input(error), frame)),
            };
            let options = capnp::message::ReaderOptions {
                traversal_limit_in_words: remaining.get(),
                ..frame.options
            };
            let source = capnp::message::Reader::new(segments, options);
            let result = self.receive_plain_reader(
                PlainSource::Unique(source, Some(&remaining)),
                messages,
                select,
            );
            // A deferred/error frame keeps the allowance consumed by the probe.
            // Successful selection owns its budget and releases this frame.
            frame.budget = capnp::message::ReadBudget::new(remaining.get());
            result
        };
        match outcome {
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
        select: impl FnOnce(questions::Key<'id>, Option<usize>) -> Option<T>,
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
        let source = capnp::message::Reader::new(segments, options);
        self.receive_plain_reader(PlainSource::Unique(source, None), messages, select)
    }

    // Keep reply validation and ownership handoff out of each receive-pump
    // expansion. Buffered and retained-frame inputs share this transition.
    #[inline(never)]
    fn receive_plain_reader<T>(
        &mut self,
        source: PlainSource<'_>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        select: impl FnOnce(questions::Key<'id>, Option<usize>) -> Option<T>,
    ) -> Result<Option<(T, alloc::boxed::Box<super::super::PlainResults>)>, Error> {
        let admitted = source.select(|root| -> Result<_, super::super::Error> {
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
            if self.outgoing.pending_for(key, &self.ledgers.questions)
                || self
                    .invocation_pipelines
                    .pending_for(key, &self.ledgers.questions)
            {
                return Ok(None);
            }
            let Some(receiver) = select(key, slot.completion_owner.map(|owner| owner.get() - 1))
            else {
                return Ok(None);
            };
            let Some((pending, content)) =
                question.prepare_plain_return(result, &mut self.ledgers.questions, true)?
            else {
                return Ok(None);
            };
            Ok(Some((content, (index, receiver, pending))))
        });
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
