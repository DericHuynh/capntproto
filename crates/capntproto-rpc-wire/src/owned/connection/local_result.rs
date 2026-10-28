//! Immutable local service results with retained graph capabilities. Their
//! placeholder wire envelope is private and is never admitted as a peer Return.
use super::*;
use capnp::{
    field_api::{FixedStruct, Schema},
    wire_capability::{Interface, Method, StreamingMethod},
};

#[must_use = "release the local result through its originating graph and message pool"]
pub struct LocalResult<'pool, 'id, const CAPS: usize> {
    pub(crate) capacity: Capacity,
    pub(crate) frame: Frame<'pool>,
    pub(crate) capabilities: Buffer<Option<clients::Client<'id>>, CAPS>,
    pub(crate) count: usize,
    pub(crate) interface: u64,
    pub(crate) method: u16,
    // Only a privately built Return with an empty capability table can carry
    // this header certificate. Payload validation follows the writer policy.
    // The frame is immutable for its entire retained lifetime.
    pub(crate) prepared_return: Option<(AnswerId, bool)>,
}
/// A borrowed local payload retains the same message-origin check used for
/// incoming capabilities; a numeric descriptor index alone is not authority.
pub struct LocalPayload<'m, 'r, 'id, const CAPS: usize> {
    content: capnp::any_pointer::Reader<'m>,
    capabilities: &'r [Option<clients::Client<'id>>],
    count: usize,
    pub(crate) interface: u64,
    pub(crate) method: u16,
}
impl<'m, 'r, 'id, const CAPS: usize> LocalPayload<'m, 'r, 'id, CAPS> {
    pub fn content(&self) -> capnp::any_pointer::Reader<'m> {
        self.content
    }
    pub fn capabilities(&self) -> &'r [Option<clients::Client<'id>>] {
        &self.capabilities[..self.count]
    }
    pub fn results<I: Interface, P: Schema, R: Schema>(
        &self,
        method: Method<I, P, R>,
    ) -> capnp::Result<R::Ref<'m>> {
        if method.interface_id() != self.interface || method.ordinal() != self.method {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::TypeMismatch));
        }
        method.read_results(self.content)
    }
    pub fn capability<I>(
        &self,
        pointer: capnp::wire_capability::Reader<'_, I>,
    ) -> Result<&'r clients::Client<'id>, super::super::Error> {
        if !pointer.belongs_to(self.content) {
            return Err(super::super::Error::WrongMessage);
        }
        self.at(pointer.index())
    }
    /// Resolve a generated result path to its retained graph client. Retaining
    /// or dispatching that client remains an explicit graph operation.
    pub fn pipeline(
        &self,
        path: pipeline::Path<'_>,
    ) -> Result<&'r clients::Client<'id>, super::super::Error> {
        self.at(path
            .resolve(self.content)
            .map_err(super::super::Error::from)?)
    }
    fn at(&self, index: u32) -> Result<&'r clients::Client<'id>, super::super::Error> {
        usize::try_from(index)
            .ok()
            .filter(|index| *index < self.count)
            .and_then(|index| self.capabilities[index].as_ref())
            .ok_or(super::super::Error::Capacity)
    }
}
impl<'pool, 'id, const CAPS: usize> LocalResult<'pool, 'id, CAPS> {
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(LocalPayload<'_, '_, 'id, CAPS>) -> R,
    ) -> Result<R, super::super::Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Return(result) = root
                .which()
                .map_err(|_| super::super::Error::WrongMessage)?
            else {
                return Err(super::super::Error::WrongMessage);
            };
            let rpc_capnp::return_::Results(payload) = result?
                .which()
                .map_err(|_| super::super::Error::WrongMessage)?
            else {
                return Err(super::super::Error::WrongMessage);
            };
            Ok(f(LocalPayload {
                content: payload?.get_content(),
                capabilities: &self.capabilities,
                count: self.count,
                interface: self.interface,
                method: self.method,
            }))
        })?
    }
    pub fn release<G: clients::Capabilities<'id>>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
    ) {
        self.frame.release(messages);
        for client in self.capabilities.into_iter().flatten() {
            graph.release(client);
        }
    }
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Complete local streaming execution with its generated method identity
    /// and an empty acknowledgment. Streaming methods expose no result schema.
    pub fn local_ack<IF: Interface, Params: Schema>(
        &mut self,
        method: StreamingMethod<IF, Params>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<LocalResult<'pool, 'id, CAPS>, Error> {
        let options = self.writer_policy.options(options);
        self.check_live()?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            calls::acknowledge(builder, capntproto_rpc_engine::ids::AnswerId::from_wire(0))
        })
        .map_err(Error::Input)?;
        Ok(LocalResult {
            prepared_return: None,
            capacity: self.message_capabilities,
            frame,
            capabilities: Buffer::Inline(core::array::from_fn(|_| None)),
            count: 0,
            interface: method.interface_id(),
            method: method.ordinal(),
        })
    }
    /// Settle one preexisting local pipeline against the exact retained result.
    /// Keep the result until all its observers settle or are abandoned. Graph
    /// reference exhaustion remains retryable instead of breaking the pipeline.
    pub fn poll_local_result(
        &mut self,
        index: usize,
        task: InvocationKey<'id>,
        result: &LocalResult<'pool, 'id, CAPS>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<Propagation, Error> {
        self.poll_local_invocation_pipeline_with(index, task, |path, runtime| {
            result
                .with(messages, |payload| match payload.pipeline(path) {
                    Ok(client) => runtime.retain(client).map(Ok).map_err(Error::Graph),
                    Err(super::super::Error::Wire(kind)) => Ok(Err(kind)),
                    Err(super::super::Error::Capacity) => Ok(Err(capnp::ErrorKind::Failed)),
                    Err(error) => Err(Error::Input(error)),
                })
                .map_err(Error::Input)?
        })
    }
    /// Build a local result without manufacturing a wire question or peer grant.
    /// Validation and every capability retain succeed before ownership is handed
    /// out. Failure releases the candidate frame and only its own new references.
    #[allow(clippy::too_many_arguments)]
    pub fn local_result<InterfaceType: Interface, Params: Schema, Results: Schema>(
        &mut self,
        method: Method<InterfaceType, Params, Results>,
        capabilities: &[&clients::Client<'id>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(FixedStruct<'_, Results>) -> capnp::Result<()>,
    ) -> Result<LocalResult<'pool, 'id, CAPS>, Error> {
        self.local_result_dynamic(
            method.interface_id(),
            method.ordinal(),
            capabilities.iter().copied().map(Some),
            messages,
            minimum,
            options,
            |field| fill(field.cast::<capnp::field_api::Struct<Results>>().init()?),
        )
    }

    /// Untyped result construction for hosted generated bindings. Null table
    /// entries keep their indices; capability references are acquired only after
    /// the entire payload validates against the supplied table. Accepting an
    /// iterator avoids a temporary table of borrowed capability references.
    #[allow(clippy::too_many_arguments)]
    pub fn local_result_dynamic<'c>(
        &mut self,
        interface: u64,
        method: u16,
        capabilities: impl ExactSizeIterator<Item = Option<&'c clients::Client<'id>>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<LocalResult<'pool, 'id, CAPS>, Error>
    where
        'id: 'c,
    {
        self.local_result_envelope(
            None,
            interface,
            method,
            capabilities,
            messages,
            minimum,
            options,
            fill,
        )
    }

    /// Prepare results for a live incoming answer. This remains private local
    /// output until complete_answer and send_answer accept it. A data-only reply
    /// whose envelope still matches at publication can reuse these bytes; late
    /// cancellation, redirection and capability grants use normal reconstruction.
    #[allow(clippy::too_many_arguments)]
    pub fn local_answer_dynamic<'c>(
        &mut self,
        answer: answers::Key<'id>,
        interface: u64,
        method: u16,
        capabilities: impl ExactSizeIterator<Item = Option<&'c clients::Client<'id>>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<LocalResult<'pool, 'id, CAPS>, Error>
    where
        'id: 'c,
    {
        let no_finish = self
            .ledgers
            .answers
            .prepare_return(answer)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
            .can_omit_finish()
            && capabilities.len() == 0;
        self.local_result_envelope(
            Some((answer.id(), no_finish)),
            interface,
            method,
            capabilities,
            messages,
            minimum,
            options,
            fill,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn local_result_envelope<'c>(
        &mut self,
        envelope: Option<(capntproto_rpc_engine::ids::AnswerId, bool)>,
        interface: u64,
        method: u16,
        capabilities: impl ExactSizeIterator<Item = Option<&'c clients::Client<'id>>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<LocalResult<'pool, 'id, CAPS>, Error>
    where
        'id: 'c,
    {
        let options = self.writer_policy.options(options);
        self.check_live()?;
        let retained = self
            .message_capabilities
            .buffer::<_, CAPS>(capabilities.len(), || None)
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        let count = u32::try_from(capabilities.len())
            .map_err(|_| Error::Input(super::super::Error::Capacity))?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            calls::dynamic_results(
                builder,
                envelope.map_or(
                    capntproto_rpc_engine::ids::AnswerId::from_wire(0),
                    |(id, _)| id,
                ),
                fill,
            )?;
            if let Some((_, no_finish)) = envelope {
                use crate::rpc_capnp::api::{Message, Return};
                builder
                    .try_get_root::<Message>()?
                    .into_field(Message::RETURN)
                    .edit()?
                    .scalar(Return::NO_FINISH_NEEDED)
                    .set(no_finish);
            }
            Ok(())
        })
        .map_err(Error::Input)?;
        let validation = if self.writer_policy == WriterPolicy::Native {
            Ok(())
        } else {
            frame
                .with(messages, |root| -> capnp::Result<()> {
                    let rpc_capnp::message::Return(result) = root.which()? else {
                        unreachable!()
                    };
                    let rpc_capnp::return_::Results(payload) = result?.which()? else {
                        unreachable!()
                    };
                    payload?.get_content().validate_capability_indices(count)
                })
                .and_then(|result| result.map_err(super::super::Error::from))
        };
        if let Err(error) = validation {
            frame.release(messages);
            return Err(Error::Input(error));
        }
        let mut result = LocalResult {
            prepared_return: envelope.filter(|_| capabilities.len() == 0),
            capacity: self.message_capabilities,
            frame,
            capabilities: retained,
            count: capabilities.len(),
            interface,
            method,
        };
        for (index, client) in capabilities.enumerate() {
            let Some(client) = client else { continue };
            match self.runtime.retain(client) {
                Ok(client) => result.capabilities[index] = Some(client),
                Err(error) => {
                    result.release(messages, &mut self.runtime);
                    return Err(Error::Graph(error));
                }
            }
        }
        Ok(result)
    }
}
