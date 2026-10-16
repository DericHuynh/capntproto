//! Fuse receipt-free reply admission and ownership handoff. General controls,
//! fixed buffers, cancellation and pipeline observers retain the ordinary path.
use super::*;

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
        if self.disconnected.is_some() || self.resolution.is_some()
            || self.ledgers.questions.is_empty()
            || self.exchanges.iter().any(|slot| {
                matches!(&slot.exchange, Some(Exchange::Returned(response))
                    if (response.requires_ordered_propagation() || slot.handoff.is_some())
                        && (self.outgoing.pending_for(response.key()) || self.invocation_pipelines.iter().any(|slot| slot.pending_for(response.key()))))
            })
        { return Ok(PlainReceived::Deferred(frame)); }
        let segments = match frame.share_segments(messages) {
            Ok(Some(segments)) => segments,
            Ok(None) => return Ok(PlainReceived::Deferred(frame)),
            Err(error) => return Err((Error::Input(error), frame)),
        };
        match self.receive_plain_segments(segments, frame.options, messages, select) {
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
        if self.disconnected.is_some() || self.resolution.is_some()
            || self.ledgers.questions.is_empty()
            || self.exchanges.iter().any(|slot| {
                matches!(&slot.exchange, Some(Exchange::Returned(response))
                    if (response.requires_ordered_propagation() || slot.handoff.is_some())
                        && (self.outgoing.pending_for(response.key()) || self.invocation_pipelines.iter().any(|slot| slot.pending_for(response.key()))))
            })
        { return Ok(None); }
        let Some(bytes) = messages
            .copy_shared(bytes)
            .map_err(|error| Error::Input(super::super::Error::Storage(error)))?
        else {
            return Ok(None);
        };
        let segments = capnp::serialize::NoAllocBufferSegments::from_buffer_exact(bytes, options)
            .map_err(|error| Error::Input(error.into()))?;
        self.receive_plain_segments(segments, options, messages, select)
    }

    // Keep reply validation and ownership handoff out of each receive-pump
    // expansion. Buffered and retained-frame inputs share this transition.
    #[inline(never)]
    fn receive_plain_segments<T>(
        &mut self,
        segments: capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
        options: capnp::message::ReaderOptions,
        messages: &mut messages::Messages<'pool, '_, '_>,
        select: impl FnOnce(questions::Key<'id>) -> Option<T>,
    ) -> Result<Option<(T, alloc::boxed::Box<super::super::PlainResults>)>, Error> {
        let source = capnp::message::Reader::new(segments, options);
        let admitted = source.try_select_pointer(
            capnp::message::ReaderOptions {
                traversal_limit_in_words: None,
                nesting_limit: i32::MAX,
            },
            |root| -> Result<_, super::super::Error> {
                let root = root.get_as::<rpc_capnp::message::Reader>()?;
                use capnp::traits::IntoInternalStructReader;
                let fields = rpc_capnp::api::MessageRef::from(root.into_internal_struct_reader());
                if fields.tag() != rpc_capnp::api::MessageUnionTag::Return {
                    return Ok(None);
                }
                let result = rpc_capnp::return_::Reader::from(
                    fields.r#return()?.into_internal_struct_reader(),
                );
                if !result.get_no_finish_needed() {
                    return Ok(None);
                }
                let id = result.get_answer_id();
                let Some((index, question)) =
                    self.exchanges.iter().enumerate().find_map(|(index, slot)| {
                        match &slot.exchange {
                            Some(Exchange::Pending(question))
                                if slot.authorized
                                    && !slot.canceled
                                    && question.key().id().to_wire() == id =>
                            {
                                Some((index, question))
                            }
                            _ => None,
                        }
                    })
                else {
                    return Ok(None);
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
                question
                    .prepare_plain_return(result, &mut self.ledgers.questions, true)
                    .map(|prepared| {
                        prepared.map(|(pending, content)| (content, (index, receiver, pending)))
                    })
            },
        );
        match admitted {
            Ok(Some((message, (index, receiver, pending)))) => {
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
