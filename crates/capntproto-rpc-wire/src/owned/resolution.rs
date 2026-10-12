//! Once-admitted Resolve tasks retain replacement ownership until graph routing
//! can progress, including reflected exports/answers and late abandoned imports.
use super::*;
use crate::clients::{BindError, EndpointOwner};
use capntproto_rpc_engine::clients;
#[cfg(test)]
mod tests;

#[must_use = "apply or explicitly release the admitted resolution"]
pub struct Resolution<'pool, 'i, 'e, 'a, I> {
    frame: Frame<'pool>,
    receipts: Receipts<'i, 'e, 'a, I, 1>,
    target: Option<imports::Key<'i>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyError {
    Input(Error),
    Binding(BindError),
    Graph(clients::Error),
}
impl core::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "owning Resolve application: {self:?}")
    }
}
impl core::error::Error for ApplyError {}
#[must_use = "retry or release the admitted resolution"]
pub struct ApplyRejected<R> {
    pub error: ApplyError,
    pub resolution: R,
}

impl<'pool, 'i, 'e, 'a, I> Resolution<'pool, 'i, 'e, 'a, I> {
    /// Apply an already-admitted Resolve to the observed capability graph.
    /// Backpressure preserves its admitted receipts; retry never admits wire
    /// grants twice. Abandoned observers still release all replacement grants.
    /// A returned barrier must go through the ordered loopback dispatcher.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn apply<'g, P: Copy + Eq, A, const N: usize, const PATH: usize, G>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
        bindings: &mut crate::clients::Imports<'_, 'g, 'i, P>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, clients::Client<'g>>,
        answers: &mut answers::Answers<
            'a,
            '_,
            answer::State<'pool, pipelines::Results<'pool, 'g, A, N>>,
        >,
        pipelines: &mut pipelines::Bindings<'_, 'g, 'a, PATH>,
    ) -> Result<Option<clients::Barrier<'g>>, ApplyRejected<Self>>
    where
        G: clients::Capabilities<'g, Peer = P>,
        G::Owner: EndpointOwner<'i, P>,
    {
        let target = self
            .target
            .and_then(|target| bindings.resolution_target(target));
        let observed = match target.map(|key| graph.retain_key(key)).transpose() {
            Ok(observed) => observed,
            Err(clients::Error::Stale) => None,
            Err(error) => {
                return Err(ApplyRejected {
                    error: ApplyError::Graph(error),
                    resolution: self,
                })
            }
        };
        let Some(observed) = observed else {
            self.release(messages, imports, exports, answers);
            return Ok(None);
        };
        let replacement = self
            .with(messages, |view| -> Result<_, ApplyError> {
                if let Some(capability) = view.replacement().map_err(ApplyError::Input)? {
                    return bindings
                        .bind_capability(capability, graph, imports, exports, answers, pipelines)
                        .map(|bound| bound.parts())
                        .map_err(ApplyError::Binding);
                }
                let exception = view
                    .exception()
                    .map_err(|error| ApplyError::Input(error.into()))?
                    .expect("admitted exception Resolve");
                let kind = match exception.get_type() {
                    Ok(rpc_capnp::exception::Type::Overloaded) => capnp::ErrorKind::Overloaded,
                    Ok(rpc_capnp::exception::Type::Disconnected) => capnp::ErrorKind::Disconnected,
                    Ok(rpc_capnp::exception::Type::Unimplemented) => {
                        capnp::ErrorKind::Unimplemented
                    }
                    _ => capnp::ErrorKind::Failed,
                };
                graph
                    .endpoint_with(clients::Location::Broken, || G::Owner::broken(kind))
                    .map(|client| (client, clients::Source::Direct, false))
                    .map_err(ApplyError::Graph)
            })
            .map_err(ApplyError::Input)
            .and_then(core::convert::identity);
        let (replacement, source, observer) = match replacement {
            Ok(value) => value,
            Err(error) => {
                graph.release(observed);
                return Err(ApplyRejected {
                    error,
                    resolution: self,
                });
            }
        };
        let replacement_key = replacement.key();
        let mut replacement = Some(replacement);
        let outcome = graph.resolve(observed.key(), &mut replacement, source);
        graph.release(observed);
        match outcome {
            Ok(barrier) => {
                self.release(messages, imports, exports, answers);
                Ok(barrier)
            }
            Err(error) => {
                graph.release(replacement.expect("rejected resolution preserves replacement"));
                if observer {
                    pipelines.discard_unexposed(replacement_key, answers);
                }
                Err(ApplyRejected {
                    error: ApplyError::Graph(error),
                    resolution: self,
                })
            }
        }
    }
    pub fn admit<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        mut make_import: impl FnMut(ImportId) -> I,
    ) -> Result<Self, Rejected<'pool, ()>> {
        Self::admit_with_receiver(frame, messages, imports, exports, answers, &mut make_import)
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn admit_with_receiver<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        receiver: &mut impl ImportReceiver<I>,
    ) -> Result<Self, Rejected<'pool, ()>> {
        let mut receipts = Receipts::new();
        let result = frame
            .with(messages, |root| {
                let rpc_capnp::message::Resolve(resolve) =
                    root.which().map_err(|_| Error::WrongMessage)?
                else {
                    return Err(Error::WrongMessage);
                };
                let resolve = resolve?;
                let mut replacement = None;
                if let rpc_capnp::resolve::Cap(descriptor) =
                    resolve.which().map_err(|_| Error::WrongMessage)?
                {
                    let descriptor = descriptor?;
                    replacement = Some(descriptor);
                    if matches!(descriptor.which(), Ok(rpc_capnp::cap_descriptor::None(()))) {
                        return Err(Error::WrongMessage);
                    }
                    receipts.initialize_descriptors(
                        core::iter::once(descriptor),
                        1,
                        imports,
                        receiver,
                    )?;
                }
                let admitted =
                    inbound::full::resolve(resolve, imports, exports, answers, receipts.slots())
                        .map_err(Error::Admission)?
                        .accept();
                let target = admitted.target();
                if let Some(descriptor) = replacement {
                    receipts.deliver(imports, receiver, core::iter::once(descriptor));
                }
                Ok(target)
            })
            .and_then(core::convert::identity);
        match result {
            Ok(target) => Ok(Self {
                frame,
                receipts,
                target,
            }),
            Err(error) => Err(Rejected {
                error,
                frame,
                owner: (),
            }),
        }
    }

    pub fn target(&self) -> Option<imports::Key<'i>> {
        self.target
    }

    /// Readers and replacement receipts are tied to this exact admitted frame.
    /// Retain/bind a replacement client before releasing the resolution task.
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(ResolutionView<'_, '_, 'i, 'e, 'a, I>) -> R,
    ) -> Result<R, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Resolve(resolve) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            Ok(f(ResolutionView {
                resolve: resolve?,
                receipts: &self.receipts,
            }))
        })?
    }

    pub fn release<E, A>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        self.receipts.release(imports, exports, answers);
        self.frame.release(messages);
    }
}

pub struct ResolutionView<'m, 'r, 'i, 'e, 'a, I> {
    resolve: rpc_capnp::resolve::Reader<'m>,
    receipts: &'r Receipts<'i, 'e, 'a, I, 1>,
}
impl<'m, 'r, 'i, 'e, 'a, I> ResolutionView<'m, 'r, 'i, 'e, 'a, I> {
    pub fn replacement(
        &self,
    ) -> Result<Option<reception::full::Capability<'r, 'm, 'i, 'e, 'a>>, Error> {
        match self.resolve.which().map_err(|_| Error::WrongMessage)? {
            rpc_capnp::resolve::Cap(descriptor) => reception::full::descriptor_capability(
                descriptor?,
                &self.receipts.storage.imports()[0],
                &self.receipts.storage.exports()[0],
                &self.receipts.storage.answers()[0],
            )
            .map(Some)
            .map_err(|error| Error::Admission(inbound::Error::Reception(error))),
            rpc_capnp::resolve::Exception(_) => Ok(None),
        }
    }
    pub fn exception(&self) -> capnp::Result<Option<rpc_capnp::exception::Reader<'m>>> {
        match self
            .resolve
            .which()
            .map_err(|_| capnp::Error::from_kind(capnp::ErrorKind::Failed))?
        {
            rpc_capnp::resolve::Exception(exception) => exception.map(Some),
            rpc_capnp::resolve::Cap(_) => Ok(None),
        }
    }
}
