//! Cancelable service output built directly in caller-owned message storage.
//! The draft stays outside the service future, retaining partial bytes and
//! capability owners until explicit completion or discard.
use super::*;
use crate::rpc_capnp::api::{Message, Payload, Return};
use capnp::{
    field_api::{FixedField, Generic, Schema, WireCapability},
    wire_capability::{Interface, Method},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftError {
    AlreadyStarted,
    Incomplete,
    Input(super::super::Error),
}
impl core::fmt::Display for DraftError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC result draft: {self:?}")
    }
}
impl core::error::Error for DraftError {}

/// A capability field and its retained owner enter the result together. No
/// foreign message index is accepted. Failed insertion returns the exact owner.
pub struct ResultCapabilities<'id, const CAPS: usize> {
    entries: Buffer<Option<clients::Client<'id>>, CAPS>,
    capacity: Capacity,
    count: usize,
}
#[must_use = "retry insertion or release the rejected capability"]
pub struct RejectedResultCapability<'id> {
    pub error: super::super::Error,
    pub client: clients::Client<'id>,
}
impl<'id, const CAPS: usize> ResultCapabilities<'id, CAPS> {
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn set<I>(
        &mut self,
        field: FixedField<'_, WireCapability<I>>,
        client: clients::Client<'id>,
    ) -> Result<(), RejectedResultCapability<'id>> {
        let index = u32::try_from(self.count).ok().filter(|_| {
            self.count
                .checked_add(1)
                .is_some_and(|needed| self.entries.ensure(self.capacity, needed, || None))
        });
        let Some(index) = index else {
            return Err(RejectedResultCapability {
                error: super::super::Error::Capacity,
                client,
            });
        };
        if let Err(error) = field.set_index(index) {
            return Err(RejectedResultCapability {
                error: error.into(),
                client,
            });
        }
        self.entries[self.count] = Some(client);
        self.count += 1;
        Ok(())
    }
    fn release(self, graph: &mut impl clients::Capabilities<'id>) {
        for client in self.entries.into_iter().flatten() {
            graph.release(client);
        }
    }
}

/// Keep this owner outside the future returned by `write_async`. Dropping that
/// future releases its byte borrow, but preserves this draft's reservation and
/// capabilities for `discard` or `finish`. A partial draft can never be sealed.
#[must_use = "finish or discard the result draft through its originating pool and graph"]
pub struct LocalResultDraft<'pool, 'id, const CAPS: usize> {
    writer: messages::Writer<'pool>,
    capabilities: ResultCapabilities<'id, CAPS>,
    options: capnp::message::ReaderOptions,
    interface: u64,
    method: u16,
    started: bool,
    length: Option<usize>,
}
impl<'pool, 'id, const CAPS: usize> LocalResultDraft<'pool, 'id, CAPS> {
    pub(super) fn reserve(
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        interface: u64,
        method: u16,
        capacity: Capacity,
    ) -> Result<Self, Error> {
        Ok(Self {
            writer: messages
                .reserve(minimum.max(16))
                .map_err(|error| Error::Input(super::super::Error::Storage(error)))?,
            capabilities: ResultCapabilities {
                entries: Buffer::Inline(core::array::from_fn(|_| None)),
                capacity,
                count: 0,
            },
            options,
            interface,
            method,
            started: false,
            length: None,
        })
    }

    /// Construct private local Return content in its final buffer. The closure
    /// may use a generated ContextService and suspend; neither the message pool
    /// nor the graph is borrowed by this future. On cancellation, failure or
    /// panic, `finish` discards partial output and all newly inserted owners.
    // This RefMut owns only the uniquely reserved output bytes, not the pool or
    // connection. Keeping it across suspension is the purpose of this API.
    #[allow(clippy::await_holding_refcell_ref)]
    pub fn write_async<'call, 'storage: 'call, F>(
        &'call mut self,
        messages: &messages::Messages<'pool, 'storage, '_>,
        fill: F,
    ) -> impl core::future::Future<Output = Result<(), DraftError>>
           + 'call
           + use<'call, 'pool, 'id, F, CAPS>
    where
        F: for<'b> core::ops::AsyncFnOnce(
                FixedField<'b, Generic<capnp::any_pointer::Owned>>,
                &'b mut ResultCapabilities<'id, CAPS>,
            ) -> capnp::Result<()>
            + 'call,
    {
        let view = if self.started {
            Err(DraftError::AlreadyStarted)
        } else {
            self.started = true;
            messages
                .write_borrowed(&mut self.writer)
                .map_err(|error| DraftError::Input(super::super::Error::Storage(error)))
        };
        let capabilities = &mut self.capabilities;
        let length = &mut self.length;
        async move {
            let mut bytes = view?;
            let outcome = async {
                let mut builder = capnp::message::FixedBuilder::from_bytes(&mut bytes[8..])?;
                let root = builder.try_init_root::<Message>()?;
                let mut result = root.into_field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(0);
                result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let payload = result.into_field(Return::RESULTS).init()?;
                fill(payload.into_field(Payload::CONTENT), capabilities).await?;
                Ok::<usize, capnp::Error>(builder.into_segment().len())
            }
            .await
            .map_err(|error| DraftError::Input(error.into()))?;
            let words = u32::try_from(outcome / 8).map_err(|_| {
                DraftError::Input(super::super::Error::Wire(
                    capnp::ErrorKind::MessageSizeOverflow,
                ))
            })?;
            bytes[..4].copy_from_slice(&0u32.to_le_bytes());
            bytes[4..8].copy_from_slice(&words.to_le_bytes());
            *length = Some(outcome + 8);
            Ok(())
        }
    }

    /// Seal only successful, fully validated output. Failure recovers both the
    /// buffer and capability owners. Success moves owners without another retain.
    pub fn finish(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut impl clients::Capabilities<'id>,
    ) -> Result<LocalResult<'pool, 'id, CAPS>, DraftError> {
        let Some(length) = self.length else {
            self.discard(messages, graph);
            return Err(DraftError::Incomplete);
        };
        let frame = match messages.seal(self.writer, length) {
            Ok(lease) => Frame::new(lease, self.options),
            Err(rejected) => {
                messages.discard(rejected.writer);
                self.capabilities.release(graph);
                return Err(DraftError::Input(super::super::Error::Storage(
                    rejected.error,
                )));
            }
        };
        let validation = frame
            .with(messages, |root| -> capnp::Result<()> {
                let rpc_capnp::message::Return(result) = root.which()? else {
                    unreachable!()
                };
                let rpc_capnp::return_::Results(payload) = result?.which()? else {
                    unreachable!()
                };
                payload?.get_content().validate_capability_indices(
                    u32::try_from(self.capabilities.count).map_err(|_| {
                        capnp::Error::from_kind(capnp::ErrorKind::MessageSizeOverflow)
                    })?,
                )
            })
            .and_then(|result| result.map_err(super::super::Error::from));
        if let Err(error) = validation {
            frame.release(messages);
            self.capabilities.release(graph);
            return Err(DraftError::Input(error));
        }
        Ok(LocalResult {
            prepared_return: None,
            capacity: self.capabilities.capacity,
            frame,
            capabilities: self.capabilities.entries,
            count: self.capabilities.count,
            interface: self.interface,
            method: self.method,
        })
    }
    pub fn discard(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut impl clients::Capabilities<'id>,
    ) {
        messages.discard(self.writer);
        self.capabilities.release(graph);
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn local_result_draft<InterfaceType: Interface, Params: Schema, Results: Schema>(
        &self,
        method: Method<InterfaceType, Params, Results>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<LocalResultDraft<'pool, 'id, CAPS>, Error> {
        self.check_live()?;
        LocalResultDraft::reserve(
            messages,
            minimum,
            options,
            method.interface_id(),
            method.ordinal(),
            self.message_capabilities,
        )
    }
}
