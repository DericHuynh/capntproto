//! Exercise count faults through the hosted adapter and inspect real wire output.
use super::*;
use crate::rpc_twoparty_capnp::Side;
use std::io;

struct PendingRead;
impl futures::AsyncRead for PendingRead {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Pending
    }
}
struct Capture(Rc<RefCell<Vec<u8>>>);
impl futures::AsyncWrite for Capture {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
struct Fixture {
    state: Rc<ConnectionState<Side>>,
    bytes: Rc<RefCell<Vec<u8>>>,
    _bootstrap: capnp::capability::Client,
    system: crate::RpcSystem<Side>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_bootstrap(None)
    }
    fn with_bootstrap(bootstrap: Option<capnp::capability::Client>) -> Self {
        let bytes = Rc::new(RefCell::new(Vec::new()));
        let network = crate::twoparty::VatNetwork::new(
            PendingRead,
            Capture(bytes.clone()),
            Side::Client,
            Default::default(),
        );
        let mut system = crate::RpcSystem::new(Box::new(network), bootstrap);
        let bootstrap = system.bootstrap(Side::Server);
        let state = system
            .connections
            .borrow()
            .states()
            .into_iter()
            .next()
            .unwrap()
            .clone();
        Self {
            state,
            bytes,
            _bootstrap: bootstrap,
            system,
        }
    }
    fn drain(&mut self) -> Vec<capnp::message::Reader<capnp::serialize::OwnedSegments>> {
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        for _ in 0..32 {
            assert!(Pin::new(&mut self.system).poll(&mut cx).is_pending());
        }
        let bytes = self.bytes.borrow();
        let mut input = &bytes[..];
        let mut messages = Vec::new();
        while let Some(message) =
            capnp::serialize::try_read_message(&mut input, Default::default()).unwrap()
        {
            messages.push(message);
        }
        messages
    }
    fn export(&self, cap: &dyn ClientHook) -> ExportId {
        let mut message = capnp::message::Builder::new_default();
        ConnectionState::write_descriptor(
            &self.state,
            cap.add_ref(),
            message.init_root(),
            &mut OutgoingFds::default(),
        )
        .unwrap()
        .unwrap()
    }
    fn saturated(&self) -> (Box<dyn ClientHook>, ExportId) {
        let cap = local_cap();
        let id = self.export(&*cap);
        self.state
            .exports
            .borrow_mut()
            .find(id)
            .unwrap()
            .references
            .retain(u32::MAX - 1)
            .unwrap();
        (cap, id)
    }
}
fn local_cap() -> Box<dyn ClientHook> {
    // A stable, settled local identity, without a promise-resolution task.
    Box::new(broken::Client::new(
        Error::failed("test capability".into()),
        true,
        0,
    ))
}

#[test]
fn completion_drops_the_method_future_before_waiting_for_its_results() {
    #[derive(Clone)]
    struct Service {
        fail: bool,
        initially_pending: bool,
        polls: Rc<Cell<usize>>,
        dropped: Rc<Cell<bool>>,
    }
    struct Execution {
        service: Service,
        // A Future is allowed to retain Results even after returning Ready.
        _results: Box<dyn ResultsHook>,
    }
    impl Future for Execution {
        type Output = capnp::Result<()>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let previous = self.service.polls.replace(self.service.polls.get() + 1);
            if previous == 0 && self.service.initially_pending {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            assert_eq!(previous, usize::from(self.service.initially_pending));
            Poll::Ready(if self.service.fail {
                Err(Error::failed("method failed".into()))
            } else {
                Ok(())
            })
        }
    }
    impl Drop for Execution {
        fn drop(&mut self) {
            assert!(!self.service.dropped.replace(true));
        }
    }
    impl ClientHook for Service {
        fn call(
            &self,
            _: u64,
            _: u16,
            _: Box<dyn ParamsHook>,
            results: Box<dyn ResultsHook>,
        ) -> Promise<(), Error> {
            Promise::from_future(Execution {
                service: self.clone(),
                _results: results,
            })
        }
        fn add_ref(&self) -> Box<dyn ClientHook> {
            Box::new(self.clone())
        }
        fn new_call(
            &self,
            _: u64,
            _: u16,
            _: Option<capnp::MessageSize>,
        ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
            unreachable!("fixture only accepts incoming calls")
        }
        fn get_ptr(&self) -> usize {
            Rc::as_ptr(&self.polls) as usize
        }
        fn get_brand(&self) -> usize {
            0
        }
        fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
            None
        }
        fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
            None
        }
        fn when_resolved(&self) -> Promise<(), Error> {
            Promise::ok(())
        }
    }
    struct Incoming(capnp::message::Builder<capnp::message::HeapAllocator>);
    impl crate::IncomingMessage for Incoming {
        fn size_in_words(&self) -> usize {
            self.0.size_in_words()
        }
        fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.0.get_root_as_reader()
        }
    }
    for no_pipeline in [false, true] {
        for initially_pending in [false, true] {
            for fail in [false, true] {
                let mut fixture = Fixture::new();
                fixture.drain();
                fixture.bytes.borrow_mut().clear();
                let service = Service {
                    fail,
                    initially_pending,
                    polls: Rc::default(),
                    dropped: Rc::default(),
                };
                let export = fixture.export(&service);
                let mut message = capnp::message::Builder::new_default();
                let mut call = message.init_root::<message::Builder>().init_call();
                call.set_question_id(42);
                call.set_no_promise_pipelining(no_pipeline);
                call.reborrow()
                    .init_target()
                    .set_imported_cap(export.to_wire());
                call.init_params();
                ConnectionState::handle_message(
                    &Rc::downgrade(&fixture.state),
                    Box::new(Incoming(message)),
                )
                .unwrap();
                let messages = fixture.drain();
                assert!(service.dropped.get(), "completed execution must be dropped");
                assert_eq!(service.polls.get(), 1 + usize::from(initially_pending));
                let mut returns = 0;
                for message in &messages {
                    if let message::Return(reply) = message
                        .get_root::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    {
                        let reply = reply.unwrap();
                        assert_eq!(reply.get_answer_id(), 42);
                        match reply.which().unwrap() {
                            return_::Results(_) => assert!(!fail),
                            return_::Exception(_) => assert!(fail),
                            _ => panic!("unexpected Return"),
                        }
                        returns += 1;
                    }
                }
                assert_eq!(returns, 1, "method completion must send exactly one Return");
                assert_eq!(fixture.state.call_words.get(), 0);
            }
        }
    }
}

#[test]
fn response_completion_keeps_question_alive_across_forwarding_and_cancellation() {
    for stage in 0..3 {
        let mut fixture = Fixture::new();
        fixture.drain();
        fixture.bytes.borrow_mut().clear();
        let message = fixture.state.new_outgoing_call(64).unwrap();
        let permit = fixture.state.admission.reserve().unwrap();
        let (reference, response) =
            Request::send_internal(&fixture.state, message, &[], false, permit).unwrap();
        let weak = Rc::downgrade(&reference);
        let id = reference.borrow().id;
        let (sender, receiver) = oneshot::channel::<()>();
        if stage != 0 {
            reference
                .borrow_mut()
                .fulfill(Promise::from_future(async move {
                    receiver.await.map_err(crate::canceled_to_error)?;
                    Err(Error::failed("forwarded failure".into()))
                }));
        }
        drop(reference);
        let mut response = Box::pin(response);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(response.as_mut().poll(&mut cx).is_pending());
        assert!(weak.upgrade().is_some());
        if stage == 2 {
            sender.send(()).unwrap();
            let Poll::Ready(Err(error)) = response.as_mut().poll(&mut cx) else {
                panic!("forwarded failure must complete the response")
            };
            assert!(error.to_string().contains("forwarded failure"));
            assert!(weak.upgrade().is_none(), "completion releases the owner");
        }
        drop(response);
        assert!(weak.upgrade().is_none(), "cancellation releases the owner");
        let finishes = fixture
            .drain()
            .iter()
            .filter(|message| {
                match message
                    .get_root::<message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                {
                    message::Finish(finish) => finish.unwrap().get_question_id() == id.to_wire(),
                    _ => false,
                }
            })
            .count();
        assert_eq!(finishes, 1);
    }
}

#[test]
fn synchronous_transport_reply_observes_the_installed_question_owner() {
    struct Reply(capnp::message::Builder<capnp::message::HeapAllocator>);
    impl crate::IncomingMessage for Reply {
        fn size_in_words(&self) -> usize {
            self.0.size_in_words()
        }
        fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.0.get_root_as_reader()
        }
    }
    struct Immediate {
        body: capnp::message::Builder<capnp::message::HeapAllocator>,
        state: Weak<ConnectionState<Side>>,
    }
    impl crate::OutgoingMessage for Immediate {
        fn get_body(&mut self) -> capnp::Result<any_pointer::Builder<'_>> {
            self.body.get_root()
        }
        fn get_body_as_reader(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.body.get_root_as_reader()
        }
        fn size_in_words(&self) -> usize {
            self.body.size_in_words()
        }
        fn take(self: Box<Self>) -> capnp::message::Builder<capnp::message::HeapAllocator> {
            self.body
        }
        fn send(
            self: Box<Self>,
        ) -> (
            Promise<(), Error>,
            Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
        ) {
            let message::Call(call) = self
                .body
                .get_root_as_reader::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("expected Call")
            };
            let mut reply = capnp::message::Builder::new_default();
            let mut ret = reply.init_root::<message::Builder>().init_return();
            ret.set_answer_id(call.unwrap().get_question_id());
            ret.set_no_finish_needed(true);
            ret.init_results()
                .get_content()
                .set_as::<capnp::text::Owned>("synchronous reply")
                .unwrap();
            ConnectionState::handle_message(&self.state, Box::new(Reply(reply))).unwrap();
            (Promise::ok(()), Rc::new(self.body))
        }
    }
    let mut fixture = Fixture::new();
    let baseline = fixture.state.snapshot().questions;
    let mut body = capnp::message::Builder::new_default();
    body.init_root::<message::Builder>().init_call();
    let message = Box::new(Immediate {
        body,
        state: Rc::downgrade(&fixture.state),
    });
    let permit = fixture.state.admission.reserve().unwrap();
    let (reference, response) =
        Request::send_internal(&fixture.state, message, &[], false, permit).unwrap();
    let response = response
        .now_or_never()
        .expect("reply delivered during send")
        .unwrap();
    assert_eq!(
        response
            .get()
            .unwrap()
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "synchronous reply"
    );
    drop((response, reference));
    fixture.drain();
    assert_eq!(fixture.state.snapshot().questions, baseline);
    assert_eq!(fixture.state.snapshot().held_responses, 0);
    assert_eq!(fixture.state.snapshot().outgoing_calls, 0);
}

#[test]
fn driven_call_and_finish_release_protocol_borrows_before_polling_io() {
    struct Inspect {
        bytes: Rc<RefCell<Vec<u8>>>,
        state: Rc<RefCell<Weak<ConnectionState<Side>>>>,
        writes: Rc<Cell<usize>>,
    }
    impl futures::AsyncWrite for Inspect {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let state = self.state.borrow().upgrade().unwrap();
            assert!(
                state.questions.try_borrow_mut().is_ok(),
                "question table borrowed during IO"
            );
            assert!(
                state.connection.try_borrow_mut().is_ok(),
                "connection borrowed during IO"
            );
            self.writes.set(self.writes.get() + 1);
            self.bytes.borrow_mut().extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    struct Reply(capnp::message::Builder<capnp::message::HeapAllocator>);
    impl crate::IncomingMessage for Reply {
        fn size_in_words(&self) -> usize {
            self.0.size_in_words()
        }
        fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.0.get_root_as_reader()
        }
    }
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let state = Rc::new(RefCell::new(Weak::new()));
    let writes = Rc::new(Cell::new(0));
    let network = crate::twoparty::VatNetwork::new(
        PendingRead,
        Inspect {
            bytes: bytes.clone(),
            state: state.clone(),
            writes: writes.clone(),
        },
        Side::Client,
        Default::default(),
    );
    let mut system = crate::RpcSystem::new(Box::new(network), None);
    let _bootstrap = system.bootstrap::<capnp::capability::Client>(Side::Server);
    let connection = system.connections.borrow().states()[0].clone();
    *state.borrow_mut() = Rc::downgrade(&connection);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(Pin::new(&mut system).poll(&mut cx).is_pending());
    let target = ConnectionState::import(&connection, ImportId::from_wire(7), false, None).unwrap();
    bytes.borrow_mut().clear();
    let before = writes.get();
    let request = target
        .new_call_with_hints(
            1,
            0,
            None,
            CallHints {
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            },
        )
        .send();
    assert!(writes.get() > before, "Call must reach IO within send");
    let call =
        capnp::serialize::read_message(bytes.borrow().as_slice(), Default::default()).unwrap();
    let message::Call(call) = call.get_root::<message::Reader>().unwrap().which().unwrap() else {
        panic!("Call")
    };
    let id = call.unwrap().get_question_id();
    let mut reply = capnp::message::Builder::new_default();
    let mut ret = reply.init_root::<message::Builder>().init_return();
    ret.set_answer_id(id);
    ret.init_results();
    ConnectionState::handle_message(&Rc::downgrade(&connection), Box::new(Reply(reply))).unwrap();
    let response = request.promise.now_or_never().unwrap().unwrap();
    bytes.borrow_mut().clear();
    let before = writes.get();
    drop(response);
    assert!(
        writes.get() > before,
        "Finish must reach IO within response drop"
    );
    let finish =
        capnp::serialize::read_message(bytes.borrow().as_slice(), Default::default()).unwrap();
    assert!(
        matches!(finish.get_root::<message::Reader>().unwrap().which().unwrap(), message::Finish(Ok(finish)) if finish.get_question_id() == id)
    );
}

struct BorrowCheckingCap {
    inner: Box<dyn ClientHook>,
    state: Weak<ConnectionState<Side>>,
}
struct EmptyIncoming;
impl crate::IncomingMessage for EmptyIncoming {
    fn size_in_words(&self) -> usize {
        0
    }
    fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        Err(Error::failed("attachment fixture has no body".into()))
    }
}
impl ClientHook for BorrowCheckingCap {
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self {
            inner: self.inner.add_ref(),
            state: self.state.clone(),
        })
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        self.inner.new_call(interface, method, size)
    }
    fn call(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.inner.call(interface, method, params, results)
    }
    fn get_brand(&self) -> usize {
        assert!(
            self.state
                .upgrade()
                .unwrap()
                .imports
                .try_borrow_mut()
                .is_ok(),
            "capability hook must run outside the import-table borrow"
        );
        self.inner.get_brand()
    }
    fn get_ptr(&self) -> usize {
        self.inner.get_ptr()
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        self.inner.get_resolved()
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        self.inner.when_more_resolved()
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        self.inner.when_resolved()
    }
}

#[test]
fn incoming_resolution_invokes_hooks_outside_import_table_borrow() {
    let mut fixture = Fixture::new();
    let replacement = BorrowCheckingCap {
        inner: local_cap(),
        state: Rc::downgrade(&fixture.state),
    };
    let export = fixture.export(&replacement);
    let promise =
        ConnectionState::import(&fixture.state, ImportId::from_wire(17), true, None).unwrap();
    let mut message = capnp::message::Builder::new_default();
    let mut resolution = message.init_root::<resolve::Builder>();
    resolution.set_promise_id(17);
    resolution.init_cap().set_receiver_hosted(export.to_wire());
    ConnectionState::handle_resolve(
        &fixture.state,
        message.get_root_as_reader().unwrap(),
        &mut IncomingFds::new(&mut EmptyIncoming),
    )
    .unwrap();
    assert!(promise.get_resolved().is_some());
    fixture.drain();
    assert!(fixture
        .state
        .imports
        .borrow()
        .slots
        .get(&ImportId::from_wire(17))
        .is_none());
}

#[test]
fn mixed_import_aliases_preserve_one_promise_resolution_obligation() {
    let mut fixture = Fixture::new();
    let id = ImportId::from_wire(17);
    let plain = ConnectionState::import(&fixture.state, id, false, None).unwrap();
    let promise = ConnectionState::import(&fixture.state, id, true, None).unwrap();
    let alias = ConnectionState::import(&fixture.state, id, false, None).unwrap();
    assert_ne!(plain.get_ptr(), promise.get_ptr());
    assert_eq!(promise.get_ptr(), alias.get_ptr());
    let mut message = capnp::message::Builder::new_default();
    let mut resolution = message.init_root::<resolve::Builder>();
    resolution.set_promise_id(17);
    resolution.init_exception().set_reason("broken remotely");
    ConnectionState::handle_resolve(
        &fixture.state,
        message.get_root_as_reader().unwrap(),
        &mut IncomingFds::new(&mut EmptyIncoming),
    )
    .unwrap();
    assert!(promise.get_resolved().is_some() && alias.get_resolved().is_some());
    assert!(ConnectionState::handle_resolve(
        &fixture.state,
        message.get_root_as_reader().unwrap(),
        &mut IncomingFds::new(&mut EmptyIncoming)
    )
    .is_err());
    assert!(ConnectionState::import(&fixture.state, id, true, None).is_err());
    assert_eq!(
        fixture
            .state
            .imports
            .borrow()
            .slots
            .get(&id)
            .unwrap()
            .import_client
            .upgrade()
            .unwrap()
            .borrow()
            .remote_references
            .count(),
        3
    );
    drop((plain, promise, alias));
    let frames = fixture.drain();
    assert!(!fixture.state.imports.borrow().slots.contains_key(&id));
    assert_eq!(
        frames
            .iter()
            .filter_map(
                |m| match m.get_root::<message::Reader>().unwrap().which().unwrap() {
                    message::Release(r) => {
                        let r = r.unwrap();
                        Some((r.get_id(), r.get_reference_count()))
                    }
                    _ => None,
                }
            )
            .collect::<Vec<_>>(),
        [(17, 3)]
    );
}

#[test]
fn rejected_descriptor_batch_rolls_back_new_and_repeated_exports() {
    let fixture = Fixture::new();
    let first = local_cap();
    let first_id = fixture.export(&*first);
    let new = local_cap();
    let (full, full_id) = fixture.saturated();
    let baseline = fixture.state.exports.borrow().len();
    for failure_at in 0..=4 {
        let mut caps = vec![
            Some(first.clone()),
            Some(new.clone()),
            Some(first.clone()),
            Some(new.clone()),
        ];
        caps.insert(failure_at, Some(full.clone()));
        let mut message = capnp::message::Builder::new_default();
        let error = ConnectionState::write_descriptors(
            &fixture.state,
            &caps,
            message.init_root(),
            &mut OutgoingFds::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(fixture.state.exports.borrow().len(), baseline);
        assert_eq!(
            fixture
                .state
                .exports
                .borrow()
                .get(first_id)
                .unwrap()
                .references
                .count(),
            1
        );
        assert_eq!(
            fixture
                .state
                .exports
                .borrow()
                .get(full_id)
                .unwrap()
                .references
                .count(),
            u32::MAX
        );
        assert!(!fixture
            .state
            .exports_by_cap
            .borrow()
            .contains_key(&new.get_ptr()));
    }
    assert!(fixture.state.release_export(first_id, 2).is_err());
    assert_eq!(
        fixture
            .state
            .exports
            .borrow()
            .get(first_id)
            .unwrap()
            .references
            .count(),
        1
    );
    fixture.state.release_export(full_id, u32::MAX - 1).unwrap();
    assert_eq!(fixture.export(&*full), full_id);
    assert_eq!(
        fixture
            .state
            .exports
            .borrow()
            .get(full_id)
            .unwrap()
            .references
            .count(),
        2
    );
}

#[test]
fn promise_wrapped_export_propagates_overflow_without_changing_descriptor() {
    let fixture = Fixture::new();
    let (full, _) = fixture.saturated();
    let wrapped = Client::from(PromiseClient::new(&fixture.state, full, None));
    let mut message = capnp::message::Builder::new_default();
    let mut descriptor = message.init_root::<cap_descriptor::Builder>();
    descriptor.set_receiver_hosted(19);
    let error = ConnectionState::write_descriptor(
        &fixture.state,
        Box::new(wrapped),
        descriptor,
        &mut OutgoingFds::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
    assert!(matches!(
        message
            .get_root_as_reader::<cap_descriptor::Reader>()
            .unwrap()
            .which()
            .unwrap(),
        cap_descriptor::ReceiverHosted(19)
    ));
}

#[test]
fn saturated_bootstrap_is_rejected_without_publishing_an_answer() {
    let cap = local_cap();
    let mut fixture = Fixture::with_bootstrap(Some(capnp::capability::Client::new(cap.clone())));
    let id = fixture.export(&*cap);
    fixture
        .state
        .exports
        .borrow_mut()
        .find(id)
        .unwrap()
        .references
        .retain(u32::MAX - 1)
        .unwrap();
    let mut message = capnp::message::Builder::new_default();
    message
        .init_root::<bootstrap::Builder>()
        .set_question_id(61);
    let error =
        ConnectionState::handle_bootstrap(&fixture.state, message.get_root_as_reader().unwrap())
            .unwrap_err();
    assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
    assert!(!fixture
        .state
        .answers
        .borrow()
        .slots
        .contains_key(&AnswerId::from_wire(61)));
    assert!(!fixture.drain().iter().any(|m| matches!(
        m.get_root::<message::Reader>().unwrap().which(),
        Ok(message::Return(_))
    )));
}

#[test]
fn every_outgoing_call_mode_rejects_before_publishing_a_question() {
    let mut fixture = Fixture::new();
    let (full, _) = fixture.saturated();
    let target =
        ConnectionState::import(&fixture.state, ImportId::from_wire(7), false, None).unwrap();
    let baseline = fixture.state.snapshot();
    for mode in 0..4 {
        let mut request = target.new_call(1, 0, None);
        request.get().set_as_capability(full.clone());
        match mode {
            0 => {
                let response = request.send();
                let error = response.promise.now_or_never().unwrap().err().unwrap();
                assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
            }
            1 => assert_eq!(
                request
                    .hook
                    .send_streaming()
                    .now_or_never()
                    .unwrap()
                    .unwrap_err()
                    .kind,
                capnp::ErrorKind::Overloaded
            ),
            2 => {
                let pipeline = request.hook.send_for_pipeline();
                let cap = pipeline.hook.get_pipelined_cap(&[]);
                assert_eq!(
                    cap.when_resolved()
                        .now_or_never()
                        .unwrap()
                        .unwrap_err()
                        .kind,
                    capnp::ErrorKind::Overloaded
                );
            }
            _ => assert!(request.hook.tail_send().is_none()),
        }
        let current = fixture.state.snapshot();
        assert_eq!(current.questions, baseline.questions);
        assert_eq!(current.outgoing_calls, baseline.outgoing_calls);
    }
    let messages = fixture.drain();
    assert!(!messages.iter().any(|m| matches!(
        m.get_root::<message::Reader>().unwrap().which(),
        Ok(message::Call(_))
    )));
}

#[test]
fn import_overflow_preserves_aliases_and_emits_one_complete_release() {
    let mut fixture = Fixture::new();
    let id = ImportId::from_wire(17);
    let first = ConnectionState::import(&fixture.state, id, false, None).unwrap();
    let alias = ConnectionState::import(&fixture.state, id, false, None).unwrap();
    let client = fixture
        .state
        .imports
        .borrow()
        .slots
        .get(&id)
        .unwrap()
        .import_client
        .upgrade()
        .unwrap();
    client
        .borrow_mut()
        .remote_references
        .receive(u32::MAX - 2)
        .unwrap();
    let rejected = ConnectionState::import(&fixture.state, id, false, None)
        .err()
        .unwrap();
    assert_eq!(rejected.kind, capnp::ErrorKind::Overloaded);
    assert_eq!(first.get_ptr(), alias.get_ptr());
    assert_eq!(client.borrow().remote_references.count(), u32::MAX);
    drop(client);
    drop(alias);
    assert!(fixture.state.imports.borrow().slots.get(&id).is_some());
    drop(first);
    assert!(fixture.state.imports.borrow().slots.get(&id).is_none());
    let messages = fixture.drain();
    let releases: Vec<_> = messages
        .iter()
        .filter_map(
            |m| match m.get_root::<message::Reader>().unwrap().which().unwrap() {
                message::Release(release) => {
                    let release = release.unwrap();
                    Some((release.get_id(), release.get_reference_count()))
                }
                _ => None,
            },
        )
        .collect();
    assert_eq!(releases, [(17, u32::MAX)]);
}

#[test]
fn saturated_result_capability_sends_an_exception_and_releases_answer_credit() {
    let mut fixture = Fixture::new();
    let (full, _) = fixture.saturated();
    let answer_id = AnswerId::from_wire(33);
    let answer = Answer::new();
    let status = answer.status.clone();
    fixture
        .state
        .answers
        .borrow_mut()
        .slots
        .insert(answer_id, answer);
    let (send, _recv) = oneshot::channel();
    let mut results = Results::new(&fixture.state, answer_id, false, send, status.clone(), None);
    results.get().unwrap().set_as_capability(full);
    let inner = results.inner.take().unwrap();
    results.results_done_fulfiller.take();
    let result =
        ResultsDone::from_results_inner(Ok(inner), Ok(()), queued::PipelineInnerSender::disabled());
    assert_eq!(result.err().unwrap().kind, capnp::ErrorKind::Overloaded);
    assert!(status.return_has_been_sent.get());
    assert_eq!(fixture.state.snapshot().incoming_call_words, 0);
    let messages = fixture.drain();
    let replies: Vec<_> = messages
        .iter()
        .filter_map(
            |m| match m.get_root::<message::Reader>().unwrap().which().unwrap() {
                message::Return(ret) => Some(ret.unwrap()),
                _ => None,
            },
        )
        .collect();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].get_answer_id(), 33);
    let return_::Exception(exc) = replies[0].which().unwrap() else {
        panic!("expected an exception, not results or cancellation")
    };
    assert_eq!(
        exc.unwrap().get_type().unwrap(),
        exception::Type::Overloaded
    );
}

#[test]
fn local_promise_resolution_waits_for_its_own_loopback_echo() {
    let mut fixture = Fixture::new();
    let mut previous = None;
    for _ in 0..2 {
        let promise = PromiseClient::new(&fixture.state, fixture._bootstrap.hook.add_ref(), None);
        let client = Client::from(promise.clone());
        promise.borrow_mut().received_call = true;
        promise.borrow_mut().resolve(Ok(local_cap()));
        assert_eq!(fixture.state.embargoes.borrow().len(), 1);
        let mut waiter = client.when_resolved();
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut waiter).poll(&mut cx).is_pending());
        let frames = fixture.drain();
        let id = frames
            .iter()
            .filter_map(|m| {
                if let message::Disembargo(d) =
                    m.get_root::<message::Reader>().unwrap().which().unwrap()
                {
                    if let disembargo::context::SenderLoopback(id) =
                        d.unwrap().get_context().which().unwrap()
                    {
                        return Some(id);
                    }
                }
                None
            })
            .last()
            .unwrap();
        let mut response = capnp::message::Builder::new_default();
        if let Some(old) = previous {
            assert_ne!(id, old);
            response
                .init_root::<disembargo::Builder>()
                .init_context()
                .set_receiver_loopback(old);
            assert!(ConnectionState::handle_disembargo(
                &fixture.state,
                response.get_root_as_reader().unwrap()
            )
            .is_err());
            assert_eq!(fixture.state.embargoes.borrow().len(), 1);
            assert!(Pin::new(&mut waiter).poll(&mut cx).is_pending());
        }
        response
            .init_root::<disembargo::Builder>()
            .init_context()
            .set_receiver_loopback(id);
        ConnectionState::handle_disembargo(&fixture.state, response.get_root_as_reader().unwrap())
            .unwrap();
        fixture.drain();
        assert!(Pin::new(&mut waiter).poll(&mut cx).is_ready());
        assert!(fixture.state.embargoes.borrow().is_empty());
        previous = Some(id);
    }
}

#[test]
fn unobserved_results_publish_answer_state_and_keep_capabilities_through_driven_io() {
    struct Inspect {
        inner: Box<dyn crate::OutgoingMessage>,
        state: Rc<ConnectionState<Side>>,
        status: Rc<AnswerStatus>,
        driven: Rc<Cell<bool>>,
    }
    impl crate::OutgoingMessage for Inspect {
        fn get_body(&mut self) -> capnp::Result<any_pointer::Builder<'_>> {
            self.inner.get_body()
        }
        fn get_body_as_reader(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.inner.get_body_as_reader()
        }
        fn size_in_words(&self) -> usize {
            self.inner.size_in_words()
        }
        fn take(self: Box<Self>) -> capnp::message::Builder<capnp::message::HeapAllocator> {
            self.inner.take()
        }
        fn send(
            self: Box<Self>,
        ) -> (
            Promise<(), Error>,
            Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
        ) {
            self.inner.send()
        }
        fn send_detached_and_drive(self: Box<Self>) {
            assert!(self.status.return_has_been_sent.get());
            assert!(self.state.answers.try_borrow_mut().is_ok());
            assert!(self.state.connection.try_borrow_mut().is_ok());
            assert!(self.state.exports.try_borrow_mut().is_ok());
            assert!(
                self.state
                    .imports
                    .borrow()
                    .slots
                    .contains_key(&ImportId::from_wire(17)),
                "returned import must stay alive through enqueue"
            );
            self.driven.set(true);
            self.inner.send_detached_and_drive();
        }
    }
    for observed in [false, true] {
        let mut fixture = Fixture::new();
        fixture.drain();
        fixture.bytes.borrow_mut().clear();
        let answer_id = AnswerId::from_wire(42);
        let answer = Answer::new();
        let status = answer.status.clone();
        fixture
            .state
            .answers
            .borrow_mut()
            .slots
            .insert(answer_id, answer);
        let (send, _recv) = oneshot::channel();
        let mut results =
            Results::new(&fixture.state, answer_id, false, send, status.clone(), None);
        let cap =
            ConnectionState::import(&fixture.state, ImportId::from_wire(17), false, None).unwrap();
        results.get().unwrap().set_as_capability(cap);
        let mut inner = results.inner.take().unwrap();
        results.results_done_fulfiller.take();
        let Some(ResultsVariant::Rpc(message, caps)) = inner.variant.take() else {
            panic!("RPC results");
        };
        let driven = Rc::new(Cell::new(false));
        inner.variant = Some(ResultsVariant::Rpc(
            Box::new(Inspect {
                inner: message,
                state: fixture.state.clone(),
                status: status.clone(),
                driven: driven.clone(),
            }),
            caps,
        ));
        let (sender, pipeline) = queued::Pipeline::new();
        let pipeline = observed.then_some(pipeline);
        assert!(matches!(
            ResultsDone::from_results_inner(Ok(inner), Ok(()), sender).unwrap(),
            ResultsCompletion::Sent
        ));
        assert_eq!(
            driven.get(),
            !observed,
            "only unobserved results transfer exclusive ownership"
        );
        assert!(status.return_has_been_sent.get());
        if observed {
            assert!(fixture
                .state
                .imports
                .borrow()
                .slots
                .contains_key(&ImportId::from_wire(17)));
        }
        drop(pipeline);
        let messages = fixture.drain();
        let mut order = Vec::new();
        for message in &messages {
            match message
                .get_root::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(reply) if reply.as_ref().unwrap().get_answer_id() == 42 => {
                    order.push("return")
                }
                message::Release(release) if release.as_ref().unwrap().get_id() == 17 => {
                    order.push("release")
                }
                _ => (),
            }
        }
        assert_eq!(order, ["return", "release"]);
    }
}
