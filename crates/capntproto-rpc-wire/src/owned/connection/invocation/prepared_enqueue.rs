//! Queue immutable native parameters through the ordinary invocation transaction.
use super::*;

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Queue a private capability-bearing Call without rebuilding its payload.
    /// The invocation retains its own bounded frame and graph references. None
    /// requests the ordinary codec for checked writers or unsupported layouts;
    /// storage rejection also permits compaction of unreachable source words.
    /// This preserves the prepared envelope's ordinary Caller destination.
    #[allow(clippy::too_many_arguments)]
    pub fn try_enqueue_prepared_with_capability_table<
        B: capnp::message::Allocator,
        T: CapabilityTable<'id> + ?Sized,
    >(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        source: &PreparedCall<B>,
        capabilities: &T,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<InvocationKey<'id>>, Error> {
        if !self.uses_native_writer() || capabilities.is_empty() || method.only_promise_pipeline {
            return Ok(None);
        }
        let Some(encoded) = source.encoded_for_sending(method).map_err(Error::Input)? else {
            return Ok(None);
        };
        let template = encoded
            .for_publication(method, options, WriterPolicy::Native)
            .map_err(Error::Input)?;
        let result = self.enqueue_frame(
            target,
            (0..capabilities.len()).map(|index| capabilities.capability(index)),
            messages,
            options,
            |messages, options| {
                encoded
                    .copy(
                        template,
                        messages,
                        capntproto_rpc_engine::ids::QuestionId::from_wire(0),
                        ImportId::from_wire(0),
                        false,
                        options,
                    )
                    .map_err(Error::Input)
            },
        );
        match result {
            Ok(key) => Ok(Some(key)),
            Err(Error::Input(super::super::super::Error::Storage(messages::Error::NoBuffer))) => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}
