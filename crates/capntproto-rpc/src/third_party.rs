// Copyright (c) 2026 ReProto contributors
// Licensed under the MIT license; see LICENSE.

//! Opaque exchanges shared by authenticated VatNetwork connections.
use capnp::capability::Promise;
use capnp::private::capability::{ClientHook, PipelineHook, ResponseHook};
use capnp::Error;
use futures::channel::oneshot;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

struct Embargo {
    receiver: Option<oneshot::Receiver<()>>,
    sender: Option<oneshot::Sender<()>>,
}
impl Embargo {
    fn new() -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            sender: Some(sender),
            receiver: Some(receiver),
        }
    }
}

pub(crate) struct Answer {
    pub response: Promise<Box<dyn ResponseHook>, Error>,
    pub pipeline: Box<dyn PipelineHook>,
}
enum Value {
    Capability(Box<dyn ClientHook>),
    Answer(RefCell<Option<oneshot::Sender<Answer>>>),
}
/// Network-owned rendezvous value for capability provision or answer adoption.
///
/// A network may store and return this opaque value, but must authenticate the
/// recipient before returning it from `Connection::complete_third_party`.
/// The registration guard returned by `await_third_party` must unregister it
/// when dropped and reject late completions. An identifier alone is not an
/// authentication mechanism. The runtime checks the exchange kind and permits
/// only one answer claim; capability acceptance retains its existing semantics.
pub struct ThirdPartyExchange {
    value: Value,
    embargoes: RefCell<HashMap<Vec<u8>, Embargo>>,
}
impl ThirdPartyExchange {
    /// Wrap authority already held by the application for an authenticated
    /// network rendezvous. The network must still authorize every completion.
    pub fn from_capability(client: capnp::capability::Client) -> Rc<Self> {
        Self::new(client.hook)
    }

    pub(crate) fn new(client: Box<dyn ClientHook>) -> Rc<Self> {
        Rc::new(Self {
            value: Value::Capability(client),
            embargoes: RefCell::new(HashMap::new()),
        })
    }

    pub(crate) fn new_answer() -> (Rc<Self>, oneshot::Receiver<Answer>) {
        let (tx, rx) = oneshot::channel();
        (
            Rc::new(Self {
                value: Value::Answer(RefCell::new(Some(tx))),
                embargoes: RefCell::new(HashMap::new()),
            }),
            rx,
        )
    }

    pub(crate) fn adopt(&self, answer: Answer) -> capnp::Result<()> {
        let Value::Answer(sender) = &self.value else {
            return Err(Error::failed(
                "ThirdPartyAnswer cannot consume a capability provision".into(),
            ));
        };
        let sender = sender
            .borrow_mut()
            .take()
            .ok_or_else(|| Error::failed("duplicate ThirdPartyAnswer completion".into()))?;
        // A canceled receiver is normal: dropping its adopted question sends Finish.
        let _ = sender.send(answer);
        Ok(())
    }

    pub(crate) fn accept(
        self: &Rc<Self>,
        embargo: Option<Vec<u8>>,
    ) -> Promise<Box<dyn ClientHook>, Error> {
        let Value::Capability(client) = &self.value else {
            return Promise::err(Error::failed(
                "Accept cannot consume an answer rendezvous".into(),
            ));
        };
        let Some(id) = embargo else {
            return Promise::ok(client.clone());
        };
        let receiver = self
            .embargoes
            .borrow_mut()
            .entry(id)
            .or_insert_with(Embargo::new)
            .receiver
            .take();
        let Some(receiver) = receiver else {
            return Promise::err(Error::failed("duplicate Accept embargo ID".into()));
        };
        // Hold the client, not the exchange: dropping the Provide must reject
        // an incomplete embargo even while an Accept waits on it.
        let client = client.clone();
        Promise::from_future(async move {
            receiver
                .await
                .map_err(|_| Error::disconnected("Provide ended before disembargo".into()))?;
            Ok(client)
        })
    }

    pub(crate) fn disembargo(&self, id: &[u8]) -> capnp::Result<()> {
        if !matches!(self.value, Value::Capability(_)) {
            return Err(Error::failed(
                "answer rendezvous has no capability embargo".into(),
            ));
        }
        let sender = self
            .embargoes
            .borrow_mut()
            .entry(id.to_vec())
            .or_insert_with(Embargo::new)
            .sender
            .take();
        let Some(sender) = sender else {
            return Err(Error::failed("duplicate Accept disembargo ID".into()));
        };
        let _ = sender.send(());
        Ok(())
    }
}
