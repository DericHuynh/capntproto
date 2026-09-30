//! Deterministic message network for exercising the real RPC engine. Identities
//! come from endpoint creation, never from a message's claimed recipient.
use capnp::{capability::Promise, message::Builder, Error};
use capnp_rpc::{
    third_party::ThirdPartyExchange, Connection, IncomingMessage, OutgoingMessage, VatNetwork,
};
use futures::{
    channel::{mpsc, oneshot},
    StreamExt,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};

struct Msg {
    body: Rc<Builder<capnp::message::HeapAllocator>>,
    #[cfg(unix)]
    fds: Vec<std::os::fd::OwnedFd>,
}
type Key = (u8, u8, Vec<u8>);
#[derive(Default)]
struct Exchange {
    values: HashMap<Key, Rc<ThirdPartyExchange>>,
    waiting: HashMap<Key, HashMap<u64, oneshot::Sender<Rc<ThirdPartyExchange>>>>,
    next_waiter: u64,
    aliases: HashMap<Key, Key>,
    retired: HashSet<Key>,
}
struct Registration(Weak<RefCell<Exchange>>, Key);
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(hub) = self.0.upgrade() {
            let mut exchange = hub.borrow_mut();
            exchange.values.remove(&self.1);
            exchange.waiting.remove(&self.1);
            exchange.retired.insert(self.1.clone());
        }
    }
}
struct Waiting(Weak<RefCell<Exchange>>, Key, u64);
impl Drop for Waiting {
    fn drop(&mut self) {
        if let Some(exchange) = self.0.upgrade() {
            let mut exchange = exchange.borrow_mut();
            if let Some(waiters) = exchange.waiting.get_mut(&self.1) {
                waiters.remove(&self.2);
                if waiters.is_empty() {
                    exchange.waiting.remove(&self.1);
                }
            }
        }
    }
}
struct Input(Msg);
impl IncomingMessage for Input {
    #[cfg(unix)]
    fn take_fds(&mut self) -> Vec<std::os::fd::OwnedFd> {
        std::mem::take(&mut self.0.fds)
    }

    fn size_in_words(&self) -> usize {
        self.0.body.size_in_words()
    }
    fn get_body(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.0.body.get_root_as_reader()
    }
}
#[derive(Default)]
pub struct ConnectionStatus {
    pub allocation_hints: Vec<u32>,
    pub answer_deadlines: Vec<oneshot::Sender<()>>,
    pub idle: bool,
    pub two_party_join: bool,
    pub third_party_answers: Option<bool>,
    pub adoptions_sent: usize,
    pub redirected_returns: usize,
    pub third_party_calls: usize,
    pub notifications: Vec<bool>,
    pub shutdowns: Vec<bool>,
    pub aborts: usize,
    pub sent: usize,
    pub calls: usize,
    pub finishes: usize,
    pub fail_body: bool,
    pub readers: usize,
    pub read_error: Option<Error>,
    shutdown_gate: Option<oneshot::Receiver<()>>,
}

struct Output {
    status: Rc<RefCell<ConnectionStatus>>,
    message: Builder<capnp::message::HeapAllocator>,
    #[cfg(unix)]
    fds: Vec<Rc<std::os::fd::OwnedFd>>,
    sender: mpsc::UnboundedSender<Msg>,
}
impl OutgoingMessage for Output {
    #[cfg(unix)]
    fn set_fds(&mut self, fds: Vec<Rc<std::os::fd::OwnedFd>>) {
        self.fds = fds;
    }

    fn get_body(&mut self) -> capnp::Result<capnp::any_pointer::Builder<'_>> {
        if self.status.borrow().fail_body {
            return Err(Error::failed("injected outgoing body failure".into()));
        }
        self.message.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.message.get_root_as_reader()
    }
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<Builder<capnp::message::HeapAllocator>>,
    ) {
        assert!(!self.status.borrow().idle, "RPC sent while idle");
        self.status.borrow_mut().sent += 1;
        match self
            .message
            .get_root_as_reader::<capnp_rpc::rpc_capnp::message::Reader>()
            .unwrap()
            .which()
        {
            Ok(capnp_rpc::rpc_capnp::message::Abort(_)) => self.status.borrow_mut().aborts += 1,
            Ok(capnp_rpc::rpc_capnp::message::Call(c)) => {
                self.status.borrow_mut().calls += 1;
                if matches!(
                    c.unwrap().get_send_results_to().which().unwrap(),
                    capnp_rpc::rpc_capnp::call::send_results_to::ThirdParty(_)
                ) {
                    self.status.borrow_mut().third_party_calls += 1;
                }
            }
            Ok(capnp_rpc::rpc_capnp::message::ThirdPartyAnswer(_)) => {
                self.status.borrow_mut().adoptions_sent += 1
            }
            Ok(capnp_rpc::rpc_capnp::message::Return(r)) => {
                if matches!(
                    r.unwrap().which().unwrap(),
                    capnp_rpc::rpc_capnp::return_::AwaitFromThirdParty(_)
                ) {
                    self.status.borrow_mut().redirected_returns += 1;
                }
            }
            Ok(capnp_rpc::rpc_capnp::message::Finish(_)) => self.status.borrow_mut().finishes += 1,
            _ => (),
        }
        let message = Rc::new(self.message);
        let result = self
            .sender
            .unbounded_send(Msg {
                body: message.clone(),
                #[cfg(unix)]
                fds: self.fds.iter().map(|fd| fd.try_clone().unwrap()).collect(),
            })
            .map_err(|_| Error::disconnected("peer gone".into()));
        (Promise::from_future(async { result }), message)
    }
    fn take(self: Box<Self>) -> Builder<capnp::message::HeapAllocator> {
        self.message
    }
    fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
}
struct Inner {
    status: Rc<RefCell<ConnectionStatus>>,
    local: u8,
    peer: u8,
    hub: Weak<RefCell<Hub>>,
    sender: mpsc::UnboundedSender<Msg>,
    receiver: RefCell<Option<mpsc::UnboundedReceiver<Msg>>>,
    exchange: Rc<RefCell<Exchange>>,
}
struct ReadLease {
    inner: Rc<Inner>,
    receiver: Option<mpsc::UnboundedReceiver<Msg>>,
}
impl Drop for ReadLease {
    fn drop(&mut self) {
        self.inner.status.borrow_mut().readers -= 1;
        *self.inner.receiver.borrow_mut() = self.receiver.take();
    }
}
#[derive(Clone)]
pub struct Endpoint(Rc<Inner>);
impl Connection<u8> for Endpoint {
    fn get_peer_vat_id(&self) -> u8 {
        self.0.peer
    }
    fn connection_id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn supports_two_party_join(&self) -> bool {
        self.0.status.borrow().two_party_join
    }
    fn set_idle(&mut self, idle: bool) {
        let mut status = self.0.status.borrow_mut();
        assert!(
            status.notifications.last() != Some(&idle),
            "duplicate idle notification"
        );
        status.idle = idle;
        status.notifications.push(idle);
    }
    fn new_outgoing_message(&mut self, size_hint: u32) -> Box<dyn OutgoingMessage> {
        assert!(
            !self.0.status.borrow().idle,
            "RPC allocated message while idle"
        );
        self.0.status.borrow_mut().allocation_hints.push(size_hint);
        Box::new(Output {
            status: self.0.status.clone(),
            message: Builder::new_default(),
            #[cfg(unix)]
            fds: Vec::new(),
            sender: self.0.sender.clone(),
        })
    }
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        let inner = self.0.clone();
        let receiver = inner.receiver.borrow_mut().take().expect("one reader");
        inner.status.borrow_mut().readers += 1;
        let mut lease = ReadLease {
            inner,
            receiver: Some(receiver),
        };
        Promise::from_future(async move {
            let value = lease.receiver.as_mut().unwrap().next().await;
            if value.is_none() {
                if let Some(error) = lease.inner.status.borrow_mut().read_error.take() {
                    return Err(error);
                }
            }
            Ok(value.map(|m| Box::new(Input(m)) as Box<dyn IncomingMessage>))
        })
    }
    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), Error> {
        self.0.status.borrow_mut().shutdowns.push(result.is_ok());
        let gate = self.0.status.borrow_mut().shutdown_gate.take();
        let sender = self.0.sender.clone();
        Promise::from_future(async move {
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            sender.close_channel();
            Ok(())
        })
    }
    fn supports_third_party(&self) -> bool {
        true
    }
    fn supports_third_party_answers(&self) -> bool {
        self.0
            .status
            .borrow()
            .third_party_answers
            .unwrap_or_else(|| {
                self.0
                    .hub
                    .upgrade()
                    .is_some_and(|h| h.borrow().answer_adoption)
            })
    }
    fn third_party_answer_timeout(&self) -> Promise<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.0.status.borrow_mut().answer_deadlines.push(tx);
        Promise::from_future(async move {
            rx.await
                .map_err(|_| Error::disconnected("test timer dropped".into()))
        })
    }
    fn introduce_to(
        &mut self,
        recipient: u8,
        mut contact: capnp::any_pointer::Builder<'_>,
        mut await_token: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<bool> {
        let Some(hub) = self.0.hub.upgrade() else {
            return Ok(false);
        };
        let mut hub = hub.borrow_mut();
        if !hub.introductions {
            return Ok(false);
        }
        hub.serial += 1;
        let token = hub.serial.to_be_bytes();
        let mut await_bytes = vec![recipient];
        await_bytes.extend_from_slice(&token);
        let mut contact_bytes = vec![self.0.peer, recipient];
        contact_bytes.extend_from_slice(&token);
        await_token.set_as::<capnp::data::Owned>(&await_bytes[..])?;
        contact.set_as::<capnp::data::Owned>(&contact_bytes[..])?;
        hub.introduction_count += 1;
        Ok(true)
    }
    fn generate_embargo_id(&mut self) -> capnp::Result<Vec<u8>> {
        let hub = self.0.hub.upgrade().unwrap();
        let mut hub = hub.borrow_mut();
        hub.serial += 1;
        Ok(hub.serial.to_be_bytes().to_vec())
    }
    fn forward_third_party_to_contact(
        &mut self,
        contact: capnp::any_pointer::Reader<'_>,
        recipient: u8,
        mut result: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<bool> {
        let hub = self.0.hub.upgrade().unwrap();
        let mut hub = hub.borrow_mut();
        if !hub.forwarding {
            return Ok(false);
        }
        let bytes = contact.get_as::<capnp::data::Reader>()?;
        if bytes.len() != 10 || bytes[1] != self.0.local {
            return Err(Error::failed("wrong forwarding recipient".into()));
        }
        if hub.reject_forward {
            return Err(Error::failed("forwarding rejected".into()));
        }
        let key = (bytes[0], bytes[1], bytes[2..].to_vec());
        let mut exchange = self.0.exchange.borrow_mut();
        let original = exchange.aliases.get(&key).cloned().unwrap_or(key);
        hub.serial += 1;
        let token = hub.serial.to_be_bytes();
        exchange
            .aliases
            .insert((bytes[0], recipient, token.to_vec()), original);
        let mut forwarded = vec![bytes[0], recipient];
        forwarded.extend_from_slice(&token);
        result.set_as::<capnp::data::Owned>(&forwarded[..])?;
        hub.forward_count += 1;
        Ok(true)
    }
    fn connect_to_introduced(
        &mut self,
        contact: capnp::any_pointer::Reader<'_>,
        mut completion: capnp::any_pointer::Builder<'_>,
    ) -> capnp::Result<Option<Box<dyn Connection<u8>>>> {
        let bytes = contact.get_as::<capnp::data::Reader>()?;
        if bytes.len() != 10 || bytes[1] != self.0.local {
            return Err(Error::failed("wrong introduction recipient".into()));
        }
        completion.set_as::<capnp::data::Owned>(&bytes[2..])?;
        let hub = self.0.hub.upgrade().unwrap();
        hub.borrow_mut().accept_count += 1;
        let local = bytes[0] == self.0.local;
        if local {
            hub.borrow_mut().local_accept_count += 1;
        }
        if hub.borrow().reject_accept {
            return Err(Error::failed("introduction rejected".into()));
        }
        if local {
            return Ok(None);
        }
        let endpoint = hub.borrow_mut().connect(self.0.local, bytes[0]);
        Ok(Some(Box::new(endpoint)))
    }
    fn await_third_party(
        &mut self,
        recipient: capnp::any_pointer::Reader<'_>,
        value: Rc<ThirdPartyExchange>,
    ) -> capnp::Result<Box<dyn std::any::Any>> {
        let bytes = recipient.get_as::<capnp::data::Reader>()?;
        if bytes.len() < 2 {
            return Err(Error::failed("bad recipient".into()));
        }
        let key = (self.0.local, bytes[0], bytes[1..].to_vec());
        let mut exchange = self.0.exchange.borrow_mut();
        if exchange.values.contains_key(&key) {
            return Err(Error::failed("duplicate provision".into()));
        }
        exchange.values.insert(key.clone(), value.clone());
        for (_, waiter) in exchange.waiting.remove(&key).unwrap_or_default() {
            let _ = waiter.send(value.clone());
        }
        Ok(Box::new(Registration(Rc::downgrade(&self.0.exchange), key)))
    }
    fn complete_third_party(
        &mut self,
        provision: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<ThirdPartyExchange>, Error> {
        self.complete_for(self.0.peer, provision)
    }
    fn complete_third_party_local(
        &mut self,
        provision: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<ThirdPartyExchange>, Error> {
        self.complete_for(self.0.local, provision)
    }
}
impl Endpoint {
    #[allow(dead_code)]
    pub fn status(&self) -> Rc<RefCell<ConnectionStatus>> {
        self.0.status.clone()
    }
    #[allow(dead_code)]
    pub fn gate_shutdown(&self) -> oneshot::Sender<()> {
        let (sender, receiver) = oneshot::channel();
        self.0.status.borrow_mut().shutdown_gate = Some(receiver);
        sender
    }
    fn complete_for(
        &self,
        recipient: u8,
        provision: capnp::any_pointer::Reader<'_>,
    ) -> Promise<Rc<ThirdPartyExchange>, Error> {
        let bytes = capnp_rpc::pry!(provision.get_as::<capnp::data::Reader>());
        let key = (self.0.local, recipient, bytes.to_vec());
        let mut exchange = self.0.exchange.borrow_mut();
        let key = exchange.aliases.get(&key).cloned().unwrap_or(key);
        if exchange.retired.contains(&key) {
            return Promise::err(Error::disconnected("provision retired".into()));
        }
        if let Some(value) = exchange.values.get(&key) {
            return Promise::ok(value.clone());
        }
        let (sender, receiver) = oneshot::channel();
        let id = exchange.next_waiter;
        exchange.next_waiter += 1;
        exchange
            .waiting
            .entry(key.clone())
            .or_default()
            .insert(id, sender);
        let waiter = Waiting(Rc::downgrade(&self.0.exchange), key, id);
        Promise::from_future(async move {
            let result = receiver
                .await
                .map_err(|_| Error::disconnected("provision gone".into()));
            drop(waiter);
            result
        })
    }

    pub fn send(&mut self, fill: impl FnOnce(capnp_rpc::rpc_capnp::message::Builder<'_>)) {
        let mut message = self.new_outgoing_message(64);
        fill(message.get_body().unwrap().init_as());
        drop(message.send());
    }
    pub async fn recv(&mut self) -> Box<dyn IncomingMessage> {
        self.receive_incoming_message().await.unwrap().unwrap()
    }
}
#[derive(Default)]
pub struct Hub {
    self_ref: Weak<RefCell<Hub>>,
    pub introductions: bool,
    pub answer_adoption: bool,
    pub introduction_count: u64,
    pub forwarding: bool,
    pub forward_count: u64,
    pub accept_count: u64,
    pub local_accept_count: u64,
    pub reject_accept: bool,
    pub reject_forward: bool,
    serial: u64,
    exchange: Rc<RefCell<Exchange>>,
    incoming: HashMap<u8, mpsc::UnboundedSender<Endpoint>>,
    outgoing: HashMap<(u8, u8), Endpoint>,
}
impl Hub {
    pub fn network(hub: &Rc<RefCell<Self>>, local: u8) -> Network {
        hub.borrow_mut().self_ref = Rc::downgrade(hub);
        let (sender, receiver) = mpsc::unbounded();
        hub.borrow_mut().incoming.insert(local, sender);
        Network {
            hub: hub.clone(),
            local,
            receiver: Rc::new(RefCell::new(Some(receiver))),
        }
    }
    pub fn connect(&mut self, local: u8, peer: u8) -> Endpoint {
        if let Some(c) = self.outgoing.get(&(local, peer)) {
            return c.clone();
        }
        let (at, ar) = mpsc::unbounded();
        let (bt, br) = mpsc::unbounded();
        let a = Endpoint(Rc::new(Inner {
            status: Rc::default(),
            local,
            peer,
            hub: self.self_ref.clone(),
            sender: at,
            receiver: RefCell::new(Some(br)),
            exchange: self.exchange.clone(),
        }));
        let b = Endpoint(Rc::new(Inner {
            status: Rc::default(),
            local: peer,
            peer: local,
            hub: self.self_ref.clone(),
            sender: bt,
            receiver: RefCell::new(Some(ar)),
            exchange: self.exchange.clone(),
        }));
        self.outgoing.insert((local, peer), a.clone());
        self.outgoing.insert((peer, local), b.clone());
        self.incoming.get(&peer).unwrap().unbounded_send(b).unwrap();
        a
    }
    #[allow(dead_code)] // Shared fixture: only handoff tests explicitly sever a pair.
    pub fn disconnect_pair(&mut self, local: u8, peer: u8) {
        self.outgoing
            .get(&(local, peer))
            .unwrap()
            .0
            .sender
            .close_channel();
        self.outgoing
            .get(&(peer, local))
            .unwrap()
            .0
            .sender
            .close_channel();
    }
    #[allow(dead_code)]
    pub fn forget_pair(&mut self, a: u8, b: u8) {
        self.outgoing.remove(&(a, b));
        self.outgoing.remove(&(b, a));
    }
    pub fn provision_count(&self) -> usize {
        self.exchange.borrow().values.len()
    }
    #[allow(dead_code)] // This shared fixture is compiled by tests without rendezvous assertions.
    pub fn waiter_count(&self) -> usize {
        self.exchange
            .borrow()
            .waiting
            .values()
            .map(HashMap::len)
            .sum()
    }
}
pub struct Network {
    hub: Rc<RefCell<Hub>>,
    local: u8,
    receiver: Rc<RefCell<Option<mpsc::UnboundedReceiver<Endpoint>>>>,
}
impl VatNetwork<u8> for Network {
    fn connect(&mut self, peer: u8) -> Option<Box<dyn Connection<u8>>> {
        (peer != self.local).then(|| {
            Box::new(self.hub.borrow_mut().connect(self.local, peer)) as Box<dyn Connection<u8>>
        })
    }
    fn accept(&mut self) -> Promise<Box<dyn Connection<u8>>, Error> {
        let cell = self.receiver.clone();
        let mut receiver = cell.borrow_mut().take().expect("one accept waiter");
        Promise::from_future(async move {
            let endpoint = receiver
                .next()
                .await
                .ok_or_else(|| Error::disconnected("network ended".into()))?;
            *cell.borrow_mut() = Some(receiver);
            Ok(Box::new(endpoint) as Box<dyn Connection<u8>>)
        })
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        Promise::from_future(futures::future::pending())
    }
}
