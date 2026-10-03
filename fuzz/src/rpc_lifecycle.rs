//! Stateful RPC over the production two-party byte-stream reader and dispatcher.
//! No socket, clock, executor thread or asserted authentication identity is used.
use crate::{lifecycle_capnp::worker, MAX_INPUT};
use capnp::{message::Builder, traits::HasTypeId};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, resolve, return_},
    rpc_twoparty_capnp::Side,
    RpcSystem,
};
use futures::{channel::oneshot, AsyncRead, AsyncWrite, Future};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

const MAX_COMMANDS: usize = 32;
const POLLS: usize = 64;

#[derive(Default)]
struct State {
    next_object: u32,
    live: BTreeSet<u32>,
    calls: Vec<(u32, u32)>,
    running: BTreeSet<u32>,
    completed: BTreeSet<u32>,
    gates: BTreeMap<u32, oneshot::Sender<bool>>,
    promise_gates: BTreeMap<u32, oneshot::Sender<bool>>,
    promises: BTreeSet<u32>,
}
struct Service {
    object: u32,
    state: Rc<RefCell<State>>,
}
impl Service {
    fn client(state: &Rc<RefCell<State>>) -> worker::Client {
        let object = {
            let mut s = state.borrow_mut();
            let id = s.next_object;
            s.next_object += 1;
            assert!(s.live.insert(id));
            id
        };
        capnp_rpc::new_client(Self {
            object,
            state: state.clone(),
        })
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        assert!(self.state.borrow_mut().live.remove(&self.object));
    }
}
struct Running {
    token: u32,
    state: Rc<RefCell<State>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        assert!(self.state.borrow_mut().running.remove(&self.token));
    }
}
struct PromiseOwner {
    token: u32,
    state: Rc<RefCell<State>>,
}
impl Drop for PromiseOwner {
    fn drop(&mut self) {
        assert!(self.state.borrow_mut().promises.remove(&self.token));
    }
}
impl worker::Server for Service {
    async fn create(
        self: Rc<Self>,
        _: worker::CreateParams,
        mut results: worker::CreateResults,
    ) -> capnp::Result<()> {
        results.get().set_cap(Self::client(&self.state));
        Ok(())
    }
    async fn echo(
        self: Rc<Self>,
        params: worker::EchoParams,
        mut results: worker::EchoResults,
    ) -> capnp::Result<()> {
        let value = params.get()?.get_value();
        self.state.borrow_mut().calls.push((self.object, value));
        results.get().set_value(value);
        Ok(())
    }
    async fn retain(
        self: Rc<Self>,
        _: worker::RetainParams,
        mut results: worker::RetainResults,
    ) -> capnp::Result<()> {
        results.get().set_cap(capnp_rpc::new_client_from_rc(self));
        Ok(())
    }
    async fn wait(
        self: Rc<Self>,
        params: worker::WaitParams,
        mut results: worker::WaitResults,
    ) -> capnp::Result<()> {
        let token = params.get()?.get_token();
        let (send, receive) = oneshot::channel();
        {
            let mut state = self.state.borrow_mut();
            if state.gates.contains_key(&token) || state.completed.contains(&token) {
                return Err(capnp::Error::failed("duplicate fixture token".into()));
            }
            state.running.insert(token);
            state.gates.insert(token, send);
        }
        let _running = Running {
            token,
            state: self.state.clone(),
        };
        let success = receive
            .await
            .map_err(|_| capnp::Error::failed("closed gate".into()))?;
        assert!(self.state.borrow_mut().completed.insert(token));
        if !success {
            return Err(capnp::Error::failed("fixture wait rejected".into()));
        }
        results.get().set_value(token);
        results.get().set_cap(capnp_rpc::new_client_from_rc(self));
        Ok(())
    }
    async fn promise(
        self: Rc<Self>,
        params: worker::PromiseParams,
        mut results: worker::PromiseResults,
    ) -> capnp::Result<()> {
        let token = params.get()?.get_token();
        let (send, receive) = oneshot::channel();
        {
            let mut state = self.state.borrow_mut();
            if state.promise_gates.contains_key(&token) || state.promises.contains(&token) {
                return Err(capnp::Error::failed(
                    "duplicate fixture promise token".into(),
                ));
            }
            state.promise_gates.insert(token, send);
            assert!(state.promises.insert(token));
        }
        let owner = PromiseOwner {
            token,
            state: self.state.clone(),
        };
        results
            .get()
            .set_cap(capnp_rpc::new_future_client(async move {
                let _owner = owner;
                match receive.await {
                    Ok(true) => Ok(capnp_rpc::new_client_from_rc(self)),
                    _ => Err(capnp::Error::failed("fixture promise rejected".into())),
                }
            }));
        Ok(())
    }
}

#[derive(Default)]
struct Io {
    input: VecDeque<u8>,
    output: Vec<u8>,
    eof: bool,
    chunk: usize,
    reader: Option<Waker>,
}
#[derive(Clone)]
struct Stream(Rc<RefCell<Io>>);
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.0.borrow_mut();
        let n = out.len().min(state.chunk).min(state.input.len());
        if n == 0 && !state.eof {
            state.reader = Some(cx.waker().clone());
            return Poll::Pending;
        }
        for byte in &mut out[..n] {
            *byte = state.input.pop_front().unwrap();
        }
        Poll::Ready(Ok(n))
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.0.borrow_mut();
        let n = input.len().min(state.chunk);
        state.output.extend_from_slice(&input[..n]);
        assert!(state.output.len() <= 256 * 1024, "unbounded RPC output");
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Export {
    Hosted(u32),
    Promise(u32),
}
fn exported(cap: cap_descriptor::Reader<'_>) -> Export {
    match cap.which().unwrap() {
        cap_descriptor::SenderHosted(id) => Export::Hosted(id),
        cap_descriptor::SenderPromise(id) => Export::Promise(id),
        _ => panic!("unexpected capability descriptor"),
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Event {
    Results {
        question: u32,
        value: u32,
        exports: Vec<Export>,
    },
    Resolve {
        promise: u32,
        result: Option<Export>,
    },
    Canceled(u32),
    Exception(u32),
    Abort,
}
struct Peer {
    io: Rc<RefCell<Io>>,
    system: Option<Pin<Box<RpcSystem<Side>>>>,
    #[cfg(test)]
    output_fault: Option<fn(&mut Vec<Event>) -> bool>,
}
impl Peer {
    fn new(state: &Rc<RefCell<State>>, profile: u8) -> Self {
        let io = Rc::new(RefCell::new(Io {
            chunk: [1, 7, 64, 4096][profile as usize % 4],
            ..Default::default()
        }));
        let network = capnp_rpc::twoparty::VatNetwork::new_with_clock(
            Stream(io.clone()),
            Stream(io.clone()),
            Side::Server,
            capnp::message::ReaderOptions {
                traversal_limit_in_words: Some(4096),
                nesting_limit: 32,
            },
            || Duration::ZERO,
        );
        let bootstrap = Service::client(state).client;
        Self {
            io,
            system: Some(Box::pin(RpcSystem::new(Box::new(network), Some(bootstrap)))),
            #[cfg(test)]
            output_fault: None,
        }
    }
    fn send(&self, fill: impl FnOnce(message::Builder<'_>)) {
        let mut message = Builder::new_default();
        fill(message.init_root());
        self.bytes(&capnp::serialize::write_message_to_words(&message));
    }
    fn bytes(&self, bytes: &[u8]) {
        let mut io = self.io.borrow_mut();
        io.input.extend(bytes);
        assert!(io.input.len() <= 256 * 1024, "unbounded RPC input");
        let reader = io.reader.take();
        drop(io);
        if let Some(reader) = reader {
            reader.wake();
        }
    }
    fn eof(&self) {
        self.io.borrow_mut().eof = true;
        self.bytes(&[]);
    }
    fn tick(&mut self) {
        // Poll the production RPC task directly. All external waits are either
        // fixture I/O or explicit oneshots; bounded polling cannot sleep/hang.
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        for _ in 0..POLLS {
            if let Some(system) = &mut self.system {
                if system.as_mut().poll(&mut cx).is_ready() {
                    self.system = None;
                }
            }
        }
    }
    fn pump(&mut self) -> Vec<Event> {
        self.tick();
        let bytes = std::mem::take(&mut self.io.borrow_mut().output);
        let mut slice = bytes.as_slice();
        let mut events = vec![];
        while !slice.is_empty() {
            let message =
                capnp::serialize::read_message_from_flat_slice(&mut slice, Default::default())
                    .unwrap();
            match message
                .get_root::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(ret) => {
                    let ret = ret.unwrap();
                    let question = ret.get_answer_id();
                    events.push(match ret.which().unwrap() {
                        return_::Results(payload) => {
                            let payload = payload.unwrap();
                            let exports = payload
                                .get_cap_table()
                                .unwrap()
                                .iter()
                                .map(exported)
                                .collect();
                            let content = payload.get_content();
                            let value = content
                                .get_as::<worker::echo_results::Reader>()
                                .map(|r| r.get_value())
                                .unwrap_or(0);
                            Event::Results {
                                question,
                                value,
                                exports,
                            }
                        }
                        return_::Canceled(()) => Event::Canceled(question),
                        return_::Exception(_) => Event::Exception(question),
                        _ => panic!("unexpected Return variant"),
                    });
                }
                message::Resolve(r) => {
                    let r = r.unwrap();
                    events.push(Event::Resolve {
                        promise: r.get_promise_id(),
                        result: match r.which().unwrap() {
                            resolve::Cap(cap) => Some(exported(cap.unwrap())),
                            resolve::Exception(_) => None,
                        },
                    });
                }
                message::Abort(_) => events.push(Event::Abort),
                _ => panic!("unexpected RPC output"),
            }
        }
        #[cfg(test)]
        if let Some(fault) = self.output_fault {
            if fault(&mut events) {
                self.output_fault = None;
            }
        }
        events
    }
    fn finish(&mut self, question: u32) {
        self.send(|m| {
            let mut f = m.init_finish();
            f.set_question_id(question);
            f.set_release_result_caps(false);
        });
    }
    fn call(&mut self, export: u32, question: u32, method: u16, value: u32) {
        self.send(|m| fill_call(m, export, question, method, value));
    }
    fn result(&mut self, question: u32) -> (u32, Vec<u32>) {
        let events = self.pump();
        let [Event::Results {
            question: id,
            value,
            exports,
        }] = events.as_slice()
        else {
            panic!("expected one successful Return for {question}: {events:?}");
        };
        assert_eq!(*id, question);
        let result = (
            *value,
            exports
                .iter()
                .map(|cap| {
                    let Export::Hosted(id) = cap else {
                        panic!("expected hosted export")
                    };
                    *id
                })
                .collect(),
        );
        self.finish(question);
        assert!(self.pump().is_empty(), "Finish produced unexpected output");
        result
    }
    fn release(&mut self, export: u32, count: u32) {
        self.send(|m| {
            let mut r = m.init_release();
            r.set_id(export);
            r.set_reference_count(count);
        });
    }
    fn pipeline(&self, parent: u32, question: u32, field: u16, value: u32) {
        self.send(|m| {
            let mut call = m.init_call();
            call.set_question_id(question);
            call.set_interface_id(worker::Client::TYPE_ID);
            call.set_method_id(1);
            let mut target = call.reborrow().init_target().init_promised_answer();
            target.set_question_id(parent);
            target.init_transform(1).get(0).set_get_pointer_field(field);
            call.init_params()
                .get_content()
                .init_as::<worker::echo_params::Builder>()
                .set_value(value);
        });
    }
}
fn fill_call(m: message::Builder<'_>, export: u32, question: u32, method: u16, value: u32) {
    let mut call = m.init_call();
    call.set_question_id(question);
    call.set_interface_id(worker::Client::TYPE_ID);
    call.set_method_id(method);
    call.reborrow().init_target().set_imported_cap(export);
    // Echo.value and Wait.token are both the first UInt32 data slot.
    call.init_params()
        .get_content()
        .init_as::<worker::echo_params::Builder>()
        .set_value(value);
}

fn take_return(events: &mut Vec<Event>, question: u32) -> Event {
    let index = events
        .iter()
        .position(|event| match event {
            Event::Results { question: id, .. } | Event::Canceled(id) | Event::Exception(id) => {
                *id == question
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("missing Return for {question}: {events:?}"));
    events.remove(index)
}

struct Object {
    export: u32,
    references: u32,
}
struct Pending {
    object: u32,
    token: u32,
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    commands: [usize; 9],
    // Creates, echoes, retains, releases, pending completions and cancellations.
    effects: [usize; 6],
    // Per-mode successful pipeline/promise histories, not just input selection.
    pipelines: [usize; 6],
    promises: [usize; 6],
    ending: u8,
    observations: Vec<(Vec<u32>, Vec<u32>, usize)>,
}
struct Machine {
    peer: Peer,
    state: Rc<RefCell<State>>,
    root: u32,
    slots: [u32; 2],
    objects: BTreeMap<u32, Object>,
    pending: BTreeMap<u32, Pending>,
    calls: Vec<(u32, u32)>,
    completed: BTreeSet<u32>,
    next_token: u32,
    summary: Summary,
}
impl Machine {
    fn new(profile: u8) -> Self {
        let state = Rc::new(RefCell::new(State::default()));
        let mut peer = Peer::new(&state, profile);
        peer.send(|m| m.init_bootstrap().set_question_id(0));
        let (_, exports) = peer.result(0);
        let [root] = exports.as_slice() else {
            panic!("bootstrap missing capability")
        };
        let mut result = Self {
            peer,
            state,
            root: *root,
            slots: [0; 2],
            objects: BTreeMap::new(),
            pending: BTreeMap::new(),
            calls: vec![],
            completed: BTreeSet::new(),
            next_token: 1,
            summary: Summary::default(),
        };
        result.create(0);
        result.create(1);
        result.observe();
        result
    }
    fn create(&mut self, slot: usize) {
        self.peer.call(self.root, 0, 0, 0);
        let (_, exports) = self.peer.result(0);
        let [export] = exports.as_slice() else {
            panic!("create missing capability")
        };
        let object = self.objects.len() as u32 + 1;
        assert!(!self
            .objects
            .values()
            .any(|o| o.references > 0 && o.export == *export));
        self.objects.insert(
            object,
            Object {
                export: *export,
                references: 1,
            },
        );
        self.slots[slot] = object;
        self.summary.effects[0] += 1;
    }
    fn choose_live(&mut self, slot: usize) -> u32 {
        let object = self.slots[slot];
        if self.objects[&object].references == 0 {
            // Re-creation is a concrete command selected relative to current
            // state, so deleting prior input commands leaves a legal history.
            self.create(slot);
        }
        self.slots[slot]
    }
    fn observe(&mut self) {
        let live: BTreeSet<_> = std::iter::once(0)
            .chain(
                self.objects
                    .iter()
                    .filter(|(id, o)| {
                        o.references > 0 || self.pending.values().any(|p| p.object == **id)
                    })
                    .map(|(&id, _)| id),
            )
            .collect();
        let running: BTreeSet<_> = self.pending.values().map(|p| p.token).collect();
        let state = self.state.borrow();
        assert_eq!(
            state.live, live,
            "capability ownership differs from reference/pending-call model"
        );
        assert_eq!(
            state.running, running,
            "pending calls leaked or canceled prematurely"
        );
        assert_eq!(
            state.completed, self.completed,
            "unexpected method completion"
        );
        assert_eq!(
            state.calls, self.calls,
            "wrong capability, value or duplicate dispatch"
        );
        self.summary.observations.push((
            live.into_iter().collect(),
            running.into_iter().collect(),
            self.calls.len(),
        ));
    }
    fn retain_export(&mut self, object: u32, exports: &[u32]) {
        let [export] = exports else {
            panic!("missing retained capability")
        };
        let entry = self.objects.get_mut(&object).unwrap();
        if entry.references > 0 {
            assert_eq!(
                entry.export, *export,
                "same live capability changed export ID"
            );
        }
        entry.export = *export;
        entry.references += 1;
        self.summary.effects[2] += 1;
        assert!(!self
            .objects
            .iter()
            .any(|(&other, o)| other != object && o.references > 0 && o.export == *export));
    }
    fn start(&mut self, object: u32) {
        let Some(question) = (1..=4).find(|q| !self.pending.contains_key(q)) else {
            return;
        };
        let token = self.next_token;
        self.next_token += 1;
        self.peer
            .call(self.objects[&object].export, question, 3, token);
        assert!(
            self.peer.pump().is_empty(),
            "pending call returned before completion"
        );
        self.pending.insert(question, Pending { object, token });
    }
    fn end(&mut self, question: u32, complete: bool) {
        let pending = self.pending.remove(&question).unwrap();
        let gate = self
            .state
            .borrow_mut()
            .gates
            .remove(&pending.token)
            .unwrap();
        if complete {
            gate.send(true)
                .expect("live call lost its completion receiver");
            let (value, exports) = self.peer.result(question);
            assert_eq!(value, pending.token);
            self.retain_export(pending.object, &exports);
            self.completed.insert(pending.token);
            self.summary.effects[4] += 1;
        } else {
            self.peer.finish(question);
            assert_eq!(self.peer.pump(), vec![Event::Canceled(question)]);
            assert!(
                gate.send(true).is_err(),
                "canceled call still owns its gate"
            );
            self.summary.effects[5] += 1;
        }
    }
    fn pipeline_history(&mut self, object: u32, mode: u8, value: u32) {
        // Questions 8..10 are private to this bounded compound command. The
        // ordinary pending-call commands can remain active on questions 1..4.
        let token = self.next_token;
        self.next_token += 1;
        self.peer.call(self.objects[&object].export, 8, 3, token);
        assert!(self.peer.pump().is_empty());
        assert!(self.state.borrow().running.contains(&token));
        for question in 9..=10 {
            self.peer
                .pipeline(8, question, u16::from(mode == 5), value + question);
        }
        assert!(
            self.peer.pump().is_empty(),
            "pipeline ran before parent completion"
        );
        assert_eq!(self.state.borrow().calls, self.calls);
        let gate = self.state.borrow_mut().gates.remove(&token).unwrap();
        let early_parent = mode == 1 || mode == 3;
        let canceled_children = match mode {
            2 => 1,
            3 => 2,
            _ => 0,
        };
        for question in 9..9 + canceled_children {
            self.peer.finish(question);
        }
        if early_parent {
            self.peer.finish(8);
        }
        let mut events = self.peer.pump();
        assert_eq!(
            self.state.borrow().calls,
            self.calls,
            "cancellation dispatched queued work"
        );
        if mode == 3 {
            assert!(
                gate.send(true).is_err(),
                "unused pipeline retained its parent"
            );
        } else {
            // Even after parent Finish, a surviving child owns the work.
            assert!(self.state.borrow().running.contains(&token));
            gate.send(mode != 4)
                .expect("pipeline lost the parent completion receiver");
            self.completed.insert(token);
        }
        events.extend(self.peer.pump());
        let parent = take_return(&mut events, 8);
        if early_parent {
            assert_eq!(parent, Event::Canceled(8));
        } else if mode == 4 {
            assert_eq!(parent, Event::Exception(8));
        } else {
            let Event::Results {
                value: returned,
                exports,
                ..
            } = parent
            else {
                panic!("pipeline parent did not return results")
            };
            assert_eq!(returned, token);
            let [Export::Hosted(export)] = exports.as_slice() else {
                panic!("missing parent capability")
            };
            self.retain_export(object, &[*export]);
        }
        for question in 9..=10 {
            let response = take_return(&mut events, question);
            if question < 9 + canceled_children {
                assert_eq!(response, Event::Canceled(question));
            } else if mode == 4 || mode == 5 {
                assert_eq!(response, Event::Exception(question));
            } else {
                assert_eq!(
                    response,
                    Event::Results {
                        question,
                        value: value + question,
                        exports: vec![]
                    }
                );
                self.calls.push((object, value + question));
            }
            if question >= 9 + canceled_children {
                self.peer.finish(question);
            }
        }
        if !early_parent {
            self.peer.finish(8);
        }
        assert!(events.is_empty(), "extra pipeline output: {events:?}");
        assert!(self.peer.pump().is_empty());
        self.summary.pipelines[mode as usize] += 1;
    }
    fn promise_history(&mut self, object: u32, mode: u8, value: u32) {
        let token = self.next_token;
        self.next_token += 1;
        self.peer.call(self.objects[&object].export, 8, 4, token);
        let mut events = self.peer.pump();
        let Event::Results { exports, .. } = take_return(&mut events, 8) else {
            panic!("missing promise result")
        };
        let [Export::Promise(promise)] = exports.as_slice() else {
            panic!("missing senderPromise descriptor")
        };
        let promise = *promise;
        assert!(events.is_empty());
        assert_eq!(self.state.borrow().promises, BTreeSet::from([token]));
        self.peer.finish(8);
        assert!(self.peer.pump().is_empty());
        let gate = self
            .state
            .borrow_mut()
            .promise_gates
            .remove(&token)
            .unwrap();
        if mode == 3 {
            self.peer.release(promise, 1);
            assert!(self.peer.pump().is_empty());
            assert!(
                gate.send(true).is_err(),
                "released promise retained its resolver"
            );
        } else {
            for question in 9..=10 {
                self.peer.call(promise, question, 1, value + question);
            }
            assert!(
                self.peer.pump().is_empty(),
                "unresolved promise dispatched a call"
            );
            assert_eq!(self.state.borrow().calls, self.calls);
            if mode == 2 || mode == 5 {
                self.peer.finish(9);
                assert_eq!(self.peer.pump(), vec![Event::Canceled(9)]);
            }
            if mode == 5 {
                self.peer.finish(10);
                assert_eq!(self.peer.pump(), vec![Event::Canceled(10)]);
            }
            if mode >= 4 {
                self.peer.release(promise, 1);
                assert!(self.peer.pump().is_empty());
                if mode == 5 {
                    assert!(
                        gate.send(true).is_err(),
                        "released promise with canceled calls retained its resolver"
                    );
                    assert!(self.state.borrow().promises.is_empty());
                    self.summary.promises[mode as usize] += 1;
                    return;
                }
                assert!(
                    self.state.borrow().promises.contains(&token),
                    "queued calls lost their released promise"
                );
            }
            gate.send(mode != 1)
                .expect("live promise lost its resolver");
            let mut events = self.peer.pump();
            if mode < 4 {
                let index = events
                    .iter()
                    .position(|e| matches!(e, Event::Resolve { .. }))
                    .expect("exported promise emitted no Resolve");
                let Event::Resolve {
                    promise: id,
                    result,
                } = events.remove(index)
                else {
                    unreachable!()
                };
                assert_eq!(id, promise, "Resolve used the wrong export ID");
                if mode == 1 {
                    assert_eq!(result, None);
                } else {
                    let Some(Export::Hosted(export)) = result else {
                        panic!("promise did not resolve to hosted capability")
                    };
                    self.retain_export(object, &[export]);
                };
            }
            for question in 9..=10 {
                if mode == 2 && question == 9 {
                    continue;
                }
                let response = take_return(&mut events, question);
                if mode == 1 {
                    assert_eq!(response, Event::Exception(question));
                } else {
                    assert_eq!(
                        response,
                        Event::Results {
                            question,
                            value: value + question,
                            exports: vec![]
                        }
                    );
                    self.calls.push((object, value + question));
                }
                self.peer.finish(question);
            }
            assert!(events.is_empty(), "extra promise output: {events:?}");
            assert!(self.peer.pump().is_empty());
            // The original promise ID remains callable after Resolve, and its
            // export reference is separate from the descriptor carried by Resolve.
            if mode < 4 {
                self.peer.call(promise, 9, 1, value);
                if mode == 1 {
                    assert_eq!(self.peer.pump(), vec![Event::Exception(9)]);
                    self.peer.finish(9);
                } else {
                    let (returned, exports) = self.peer.result(9);
                    assert_eq!(returned, value);
                    assert!(exports.is_empty());
                    self.calls.push((object, value));
                }
                self.peer.release(promise, 1);
            }
            assert!(self.peer.pump().is_empty());
        }
        assert!(
            self.state.borrow().promises.is_empty(),
            "promise future retained its owner"
        );
        self.summary.promises[mode as usize] += 1;
    }
    fn command(&mut self, bytes: [u8; 4]) {
        let [kind, pick, lo, hi] = bytes;
        let kind = kind as usize % 9;
        let slot = pick as usize % 2;
        let value = u32::from(u16::from_le_bytes([lo, hi]));
        self.summary.commands[kind] += 1;
        match kind {
            0 => {
                // Create or re-export a live object.
                if self.objects[&self.slots[slot]].references == 0 {
                    self.create(slot);
                } else {
                    let object = self.slots[slot];
                    self.peer.call(self.objects[&object].export, 0, 2, 0);
                    let (_, exports) = self.peer.result(0);
                    self.retain_export(object, &exports);
                }
            }
            1 | 2 => {
                let object = self.choose_live(slot);
                self.peer.call(
                    self.objects[&object].export,
                    0,
                    if kind == 1 { 1 } else { 2 },
                    value,
                );
                let (result, exports) = self.peer.result(0);
                if kind == 1 {
                    assert_eq!(result, value);
                    assert!(exports.is_empty());
                    self.calls.push((object, value));
                    self.summary.effects[1] += 1;
                } else {
                    self.retain_export(object, &exports);
                }
            }
            3 => {
                let object = self.choose_live(slot);
                let entry = self.objects.get_mut(&object).unwrap();
                let count = value % (entry.references + 1);
                self.peer.release(entry.export, count);
                entry.references -= count;
                self.summary.effects[3] += usize::from(count > 0);
                assert!(self.peer.pump().is_empty());
            }
            4 => {
                let object = self.choose_live(slot);
                self.start(object);
            }
            5 | 6 => {
                if !self.pending.is_empty() {
                    let question = *self
                        .pending
                        .keys()
                        .nth(pick as usize % self.pending.len())
                        .unwrap();
                    self.end(question, kind == 5);
                }
            }
            7 => {
                let object = self.choose_live(slot);
                self.pipeline_history(object, pick / 2 % 6, value);
            }
            8 => {
                let object = self.choose_live(slot);
                self.promise_history(object, pick / 2 % 6, value);
            }
            _ => unreachable!(),
        }
        self.observe();
    }
    fn ending(&mut self, ending: u8, tail: &[u8]) {
        self.summary.ending = ending % 8;
        match ending % 8 {
            0 => {
                while let Some(&question) = self.pending.keys().next() {
                    self.end(question, false);
                }
                for object in self.objects.values_mut().filter(|o| o.references > 0) {
                    self.peer.release(object.export, object.references);
                    object.references = 0;
                }
                assert!(self.peer.pump().is_empty());
                self.observe();
                self.peer.release(self.root, 1);
                assert!(self.peer.pump().is_empty());
            }
            1..=4 => {
                match ending % 8 {
                    1 => {
                        let object = self.choose_live(0);
                        let entry = &self.objects[&object];
                        self.peer.release(entry.export, entry.references + 1);
                    }
                    2 => self.peer.release(u32::MAX, 0),
                    3 => {
                        if self.pending.is_empty() {
                            let object = self.choose_live(0);
                            self.start(object);
                        }
                        let question = *self.pending.keys().next().unwrap();
                        self.peer.call(self.root, question, 1, 99);
                    }
                    // High question IDs have legacy pipeline-only semantics;
                    // use an unallocated ordinary ID for this rejection case.
                    4 => self.peer.send(|m| {
                        m.init_return().set_answer_id(1024);
                    }),
                    _ => unreachable!(),
                }
                assert_eq!(
                    self.peer.pump(),
                    vec![Event::Abort],
                    "invalid lifecycle operation {ending} was not rejected"
                );
                assert_eq!(
                    self.state.borrow().calls,
                    self.calls,
                    "invalid operation dispatched a method"
                );
            }
            5..=7 => {
                let mut message = Builder::new_default();
                fill_call(message.init_root(), self.root, 0, 1, 42);
                let mut bytes = capnp::serialize::write_message_to_words(&message);
                match ending % 8 {
                    5 => bytes = tail.to_vec(),
                    6 => bytes.truncate(tail.first().copied().unwrap_or(0) as usize % bytes.len()),
                    7 => {
                        for (i, byte) in tail.iter().enumerate() {
                            let offset = i % bytes.len();
                            bytes[offset] ^= byte;
                        }
                    }
                    _ => unreachable!(),
                }
                self.peer.bytes(&bytes);
                self.peer.eof();
                // Arbitrary terminal bytes can legitimately produce variants
                // beyond the fixture's normal Returns (e.g. Unimplemented).
                // Check their framing without imposing the valid-prefix oracle.
                self.peer.tick();
                let bytes = std::mem::take(&mut self.peer.io.borrow_mut().output);
                let mut slice = bytes.as_slice();
                while !slice.is_empty() {
                    let message = capnp::serialize::read_message_from_flat_slice(
                        &mut slice,
                        Default::default(),
                    )
                    .unwrap();
                    assert!(message
                        .get_root::<message::Reader>()
                        .unwrap()
                        .which()
                        .is_ok());
                }
                if ending % 8 == 6 {
                    assert_eq!(
                        self.state.borrow().calls,
                        self.calls,
                        "truncated Call reached dispatch"
                    );
                }
            }
            _ => unreachable!(),
        }
    }
}

/// Header: I/O profile, terminal fault, command count modulo 33. Each command
/// is (kind, state-relative selector, little-endian value); absent bytes are zero.
/// Remaining bytes mutate only the terminal frame, after the valid history.
pub fn check(input: &[u8]) {
    let _ = run(input);
}
pub fn run(input: &[u8]) -> Summary {
    run_with_feedback(input, |_| {})
}

/// Progress from successful operations, shared with the AFL++ IJON observer.
/// Counts are bounded by the command budget; no fuzz-engine dependency enters
/// the oracle and ordinary/libFuzzer replays use exactly the same assertions.
pub struct Progress {
    pub state: u32,
    pub completed: usize,
    pub effects: usize,
}
pub fn run_with_feedback(input: &[u8], mut feedback: impl FnMut(Progress)) -> Summary {
    let input = &input[..input.len().min(MAX_INPUT)];
    let get = |i| input.get(i).copied().unwrap_or(0);
    let count = get(2) as usize % (MAX_COMMANDS + 1);
    let mut machine = Machine::new(get(0));
    for i in 0..count {
        machine.command(std::array::from_fn(|j| get(3 + i * 4 + j)));
        let state = machine.state.borrow();
        feedback(Progress {
            state: (state.live.len().min(63) as u32)
                | ((state.running.len().min(63) as u32) << 6)
                | ((state.promises.len().min(63) as u32) << 12)
                | ((machine.pending.len().min(63) as u32) << 18),
            completed: state.completed.len(),
            effects: machine.summary.effects.iter().sum(),
        });
    }
    machine.ending(get(1), input.get(3 + count * 4..).unwrap_or_default());
    let state = machine.state.clone();
    let summary = std::mem::take(&mut machine.summary);
    drop(machine);
    assert!(
        state.borrow().live.is_empty(),
        "RPC teardown retained capabilities"
    );
    assert!(
        state.borrow().running.is_empty(),
        "RPC teardown retained pending calls"
    );
    assert!(
        state.borrow().gates.values().all(|gate| gate.is_canceled()),
        "RPC teardown retained completion receivers"
    );
    assert!(
        state.borrow().promises.is_empty(),
        "RPC teardown retained promise futures"
    );
    assert!(
        state
            .borrow()
            .promise_gates
            .values()
            .all(|gate| gate.is_canceled()),
        "RPC teardown retained promise resolvers"
    );
    summary
}

pub fn seeds() -> Vec<Vec<u8>> {
    let histories: &[&[[u8; 4]]] = &[
        &[],
        &[[1, 0, 42, 0], [2, 0, 0, 0], [3, 0, 2, 0], [0, 0, 0, 0]],
        &[[4, 0, 0, 0], [3, 0, 1, 0], [5, 0, 0, 0], [1, 0, 7, 0]],
        &[[4, 1, 0, 0], [3, 1, 1, 0], [6, 0, 0, 0], [0, 1, 0, 0]],
        &[
            [4, 0, 0, 0],
            [4, 1, 0, 0],
            [6, 0, 0, 0],
            [4, 0, 0, 0],
            [5, 1, 0, 0],
        ],
        &[[4, 0, 0, 0], [4, 1, 0, 0], [3, 0, 1, 0], [3, 1, 1, 0]][..],
    ];
    let mut histories = histories.iter().map(|h| h.to_vec()).collect::<Vec<_>>();
    for mode in 0..6 {
        histories.push(vec![[4, 1, 0, 0], [7, mode * 2, 42, 0], [5, 0, 0, 0]]);
    }
    for mode in 0..6 {
        histories.push(vec![
            [4, 0, 0, 0],
            [8, mode * 2 + 1, 255, 255],
            [6, 0, 0, 0],
        ]);
    }
    // Seed 1 is also the Cargo gate's saved-input replay control.
    let mut replay = vec![0, 0, 14, 4, 0, 0, 0];
    for kind in [7, 8] {
        for mode in 0..6 {
            replay.extend_from_slice(&[kind, mode * 2, 42, 0]);
        }
    }
    replay.extend_from_slice(&[6, 0, 0, 0]);
    let mut seeds = vec![vec![], replay];
    for profile in 0..4 {
        for ending in 0..8 {
            for history in &histories {
                let mut bytes = vec![profile, ending, history.len() as u8];
                bytes.extend(history.iter().flatten());
                seeds.push(bytes);
            }
        }
    }
    for kind in 0..9 {
        let mut bytes = vec![3, 0, MAX_COMMANDS as u8];
        for i in 0..MAX_COMMANDS {
            bytes.extend_from_slice(&[kind, i as u8, 255, 255]);
        }
        seeds.push(bytes);
    }
    // Raw terminal messages can invoke the fixture too. Repeated application
    // tokens must return an error rather than panic inside the test server.
    for profile in 0..4 {
        let mut bytes = vec![profile, 5, 0];
        for question in [8, 9] {
            let mut message = Builder::new_default();
            fill_call(message.init_root(), 0, question, 4, 77);
            bytes.extend(capnp::serialize::write_message_to_words(&message));
        }
        seeds.push(bytes);
    }
    seeds.push(vec![255; MAX_INPUT]);
    seeds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_histories_cover_commands_endings_and_replay() {
        let mut commands = [0; 9];
        let mut effects = [0; 6];
        let mut pipelines = [0; 6];
        let mut promises = [0; 6];
        let mut endings = BTreeSet::new();
        for (index, input) in seeds().iter().enumerate() {
            let summary = std::panic::catch_unwind(|| run(input))
                .unwrap_or_else(|_| panic!("RPC seed {index}: {input:?}"));
            for (total, count) in commands.iter_mut().zip(summary.commands) {
                *total += count;
            }
            for (total, count) in effects.iter_mut().zip(summary.effects) {
                *total += count;
            }
            for (total, count) in pipelines.iter_mut().zip(summary.pipelines) {
                *total += count;
            }
            for (total, count) in promises.iter_mut().zip(summary.promises) {
                *total += count;
            }
            endings.insert(summary.ending);
            let mut first = Vec::new();
            let observed =
                run_with_feedback(input, |p| first.push((p.state, p.completed, p.effects)));
            let mut second = Vec::new();
            assert_eq!(
                observed,
                run_with_feedback(input, |p| second.push((p.state, p.completed, p.effects)))
            );
            assert_eq!(
                first, second,
                "persistent feedback changed for seed {index}"
            );
            assert_eq!(
                summary, observed,
                "feedback changed oracle semantics for seed {index}"
            );
        }
        assert!(commands.iter().all(|&count| count > 0));
        assert!(effects.iter().all(|&count| count > 0));
        assert!(pipelines.iter().all(|&count| count > 0));
        assert!(promises.iter().all(|&count| count > 0));
        assert_eq!(endings.len(), 8);
    }

    #[test]
    fn removing_commands_and_truncating_inputs_preserve_valid_setup() {
        for input in seeds().into_iter().filter(|s| s.len() < 100) {
            for end in 0..input.len() {
                check(&input[..end]);
            }
            let count = input.get(2).copied().unwrap_or(0) as usize;
            for i in 0..count {
                let mut shorter = input.clone();
                shorter.drain(3 + i * 4..3 + (i + 1) * 4);
                shorter[2] -= 1;
                check(&shorter);
            }
        }
    }

    #[test]
    fn ownership_dispatch_and_pending_oracles_detect_wrong_observations() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let mut machine = Machine::new(3);
        let object = machine.slots[0];
        machine.objects.get_mut(&object).unwrap().references = 0;
        assert!(catch_unwind(AssertUnwindSafe(|| machine.observe())).is_err());
        machine.objects.get_mut(&object).unwrap().references = 1;
        machine.calls.push((object, 99));
        assert!(catch_unwind(AssertUnwindSafe(|| machine.observe())).is_err());
        machine.calls.clear();
        machine.start(object);
        let pending = machine.pending.remove(&1).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(|| machine.observe())).is_err());
        machine.pending.insert(1, pending);
        machine.observe();
        machine.ending(0, &[]);
    }

    #[test]
    fn pipeline_and_resolve_oracles_reject_corrupted_observations() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        type Fault = fn(&mut Vec<Event>) -> bool;
        let cases: [(u8, u8, Fault); 6] = [
            (7, 0, |events| {
                for event in events {
                    if let Event::Results {
                        question: 9, value, ..
                    } = event
                    {
                        *value ^= 1;
                        return true;
                    }
                }
                false
            }),
            (7, 0, |events| {
                if events
                    .iter()
                    .any(|e| matches!(e, Event::Results { question: 9, .. }))
                {
                    events.push(Event::Results {
                        question: 9,
                        value: 51,
                        exports: vec![],
                    });
                    return true;
                }
                false
            }),
            (7, 2, |events| {
                for event in events {
                    if *event == Event::Canceled(8) {
                        *event = Event::Exception(8);
                        return true;
                    }
                }
                false
            }),
            (8, 0, |events| {
                for event in events {
                    if let Event::Resolve { promise, .. } = event {
                        *promise += 1;
                        return true;
                    }
                }
                false
            }),
            (8, 0, |events| {
                for event in events {
                    if let Event::Resolve { result, .. } = event {
                        *result = Some(Export::Hosted(u32::MAX));
                        return true;
                    }
                }
                false
            }),
            (8, 2, |events| {
                for event in events {
                    if let Event::Resolve { result, .. } = event {
                        *result = Some(Export::Hosted(0));
                        return true;
                    }
                }
                false
            }),
        ];
        for (kind, selector, fault) in cases {
            let mut machine = Machine::new(3);
            machine.peer.output_fault = Some(fault);
            assert!(
                catch_unwind(AssertUnwindSafe(|| machine.command([kind, selector, 42, 0])))
                    .is_err()
            );
            assert!(
                machine.peer.output_fault.is_none(),
                "fault never reached its intended output"
            );
            let state = machine.state.clone();
            drop(machine);
            let state = state.borrow();
            assert!(state.live.is_empty());
            assert!(state.running.is_empty());
            assert!(state.promises.is_empty());
        }
    }

    #[test]
    fn io_fragmentation_preserves_lifecycle_observations() {
        // Includes the mixed pipeline/Resolve replay and every terminal action.
        for mut input in seeds().into_iter().filter(|s| s.len() >= 3 && s[0] == 0) {
            let expected = run(&input);
            for profile in 1..4 {
                input[0] = profile;
                assert_eq!(
                    run(&input),
                    expected,
                    "chunking changed RPC behavior: {input:?}"
                );
            }
        }
    }

    #[test]
    fn repeated_promise_tokens_preserve_the_original_resolver() {
        let mut machine = Machine::new(3);
        machine.peer.call(machine.root, 8, 4, 77);
        let mut events = machine.peer.pump();
        assert!(matches!(take_return(&mut events, 8), Event::Results { .. }));
        assert!(events.is_empty());
        machine.peer.call(machine.root, 9, 4, 77);
        assert_eq!(machine.peer.pump(), vec![Event::Exception(9)]);
        assert_eq!(machine.state.borrow().promises, BTreeSet::from([77]));
        assert!(!machine.state.borrow().promise_gates[&77].is_canceled());
        let state = machine.state.clone();
        drop(machine);
        assert!(state.borrow().promises.is_empty());
        assert!(state.borrow().promise_gates[&77].is_canceled());
        assert!(state.borrow().live.is_empty());
    }
}
