use super::*;
use capntproto_rpc_engine::{
    connection as storage,
    ids::{ImportId, WireId},
    imports, io,
};
use capntproto_rpc_wire::{calls, owned::Frame, rpc_capnp};
use futures::FutureExt;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

type TestPeer<'p, 'id, 's> = Peer<'p, 'id, 's, (), (), u8, 8, 8>;
type TestCalls<'s, 'wait, 'p, 'id> = Calls<'s, 'wait, 'p, 'id, (), 8, 8>;
type Service = dyn Fn(
    u16,
    capability::Params<any_pointer::Owned>,
    capability::Results<any_pointer::Owned>,
) -> capability::DispatchCallResult;
#[derive(Clone)]
struct Server(Rc<Service>);
impl capability::Server for Server {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as *const () as usize
    }
    fn dispatch_call(
        self,
        _: u64,
        method: u16,
        params: capability::Params<any_pointer::Owned>,
        results: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        (self.0)(method, params, results)
    }
}
fn server(
    f: impl Fn(
            u16,
            capability::Params<any_pointer::Owned>,
            capability::Results<any_pointer::Owned>,
        ) -> capability::DispatchCallResult
        + 'static,
) -> Rc<dyn ClientHook> {
    Rc::new(crate::local::Client::new(Server(Rc::new(f))))
}
fn local<'p, 'id>(
    driver: &mut TestPeer<'p, 'id, '_>,
    hooks: &mut ClientHooks<'id>,
    hook: Rc<dyn ClientHook>,
) -> capability::Client {
    let client = driver
        .runtime
        .endpoint(Route::local(hook), Location::Local)
        .unwrap_or_else(|e| panic!("{}", e.error));
    let exported = hooks.export_client(&mut driver.runtime, &client).unwrap();
    driver.runtime.release(client);
    exported
}
fn import<'p, 'id>(
    runtime: &mut Runtime<'p, 'id, '_, (), u8, Rc<dyn ClientHook>, 8, 8>,
    hook: &dyn ClientHook,
) -> capnp::Result<Client<'id>> {
    runtime
        .endpoint(Route::local(Rc::from(hook.add_ref())), Location::Local)
        .map_err(|e| wire_error(e.error))
}
fn remote<'p, 'id>(
    driver: &mut TestPeer<'p, 'id, '_>,
    hooks: &mut ClientHooks<'id>,
) -> capability::Client {
    let mut slots = [imports::ReceiveSlot::new(ImportId::from_wire(7), ())];
    driver
        .ledgers
        .imports
        .prepare_receive(&mut slots)
        .unwrap()
        .commit();
    let reference = slots[0].take_local().unwrap();
    let cap = driver
        .imports
        .bind(&mut driver.runtime, &mut driver.ledgers.imports, &reference)
        .unwrap();
    driver.ledgers.imports.release(reference);
    let client = hooks.export_client(&mut driver.runtime, &cap).unwrap();
    driver.runtime.release(cap);
    client
}
fn with_peer(
    f: impl for<'p, 'id, 's, 'm, 'b> FnOnce(TestPeer<'p, 'id, 's>, &mut messages::Messages<'p, 'm, 'b>),
) {
    let mut buffers = messages::GrowingBuffers::new(32, 1 << 20, 4 << 20);
    messages::with_growing_messages(
        &mut buffers,
        storage::Growth {
            initial: 0,
            maximum: 8,
        },
        |messages| {
            storage::with_domain(|domain| {
                let mut slots =
                    connection::FixedStorage::<(), (), u8, Rc<dyn ClientHook>, 8, 8, 24, 96>::new();
                f(slots.bind(domain, 1).unwrap(), messages);
            })
        },
    )
    .unwrap();
}
fn tick<'p, 'id>(
    calls: &mut TestCalls<'_, '_, 'p, 'id>,
    hooks: &mut ClientHooks<'id>,
    driver: &mut TestPeer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
    cx: &mut Context<'_>,
) -> Poll<capnp::Result<Progress>> {
    calls.poll(
        hooks,
        driver,
        messages,
        queue,
        128,
        Default::default(),
        &mut [0; 24],
        cx,
        NonZeroUsize::new(1).unwrap(),
        import,
    )
}
fn drive<'p, 'id>(
    calls: &mut TestCalls<'_, '_, 'p, 'id>,
    hooks: &mut ClientHooks<'id>,
    driver: &mut TestPeer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    for _ in 0..256 {
        if let Poll::Ready(Err(error)) = tick(
            calls,
            hooks,
            driver,
            messages,
            queue,
            &mut Context::from_waker(Waker::noop()),
        ) {
            assert_eq!(error.kind, capnp::ErrorKind::Overloaded, "{error}");
        }
    }
}
fn teardown<'p, 'id>(
    calls: TestCalls<'_, '_, 'p, 'id>,
    hooks: ClientHooks<'id>,
    mut driver: TestPeer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    calls.terminate(&mut driver, messages);
    hooks.close(
        &mut driver.runtime,
        Error::disconnected("test close".into()),
    );
    let mut closing = driver.terminate(queue);
    let mut done = false;
    for _ in 0..1024 {
        if matches!(
            closing
                .poll(messages, NonZeroUsize::new(32).unwrap())
                .unwrap(),
            connection::Termination::Done
        ) {
            done = true;
            break;
        }
    }
    assert!(done, "terminal cleanup");
    let writers: Vec<_> = (0..32).map(|_| messages.reserve(128).unwrap()).collect();
    for writer in writers {
        messages.discard(writer);
    }
}
fn ready<F: Future + Unpin>(future: &mut F) -> F::Output {
    match Pin::new(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("pending completion"),
    }
}
fn request(
    client: &capability::Client,
    method: u16,
) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
    client.new_call(7, method, None)
}
fn ok() -> capability::DispatchCallResult {
    capability::DispatchCallResult {
        promise: Promise::ok(()),
        is_streaming: false,
        allow_cancellation: true,
    }
}

#[test]
fn consumed_hosted_reply_waits_for_its_completion_binding_before_new_admission() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(8, 8);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = seen.clone();
        let client = local(
            &mut driver,
            &mut hooks,
            server(move |method, _, _| {
                recorded.borrow_mut().push(method);
                ok()
            }),
        );
        let mut first = request(&client, 1).send();
        let mut second = request(&client, 2).send();
        let mut calls = Calls::new(1, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut observed_cleanup_gap = false;
        for _ in 0..256 {
            if calls.entries.iter().all(Option::is_none) && !calls.invocations.has_capacity() {
                observed_cleanup_gap = true;
            }
            if let Poll::Ready(result) = tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut Context::from_waker(Waker::noop()),
            ) {
                result.expect("temporary completion pressure must not fail the connection");
            }
        }
        assert!(observed_cleanup_gap);
        assert_eq!(*seen.borrow(), [1, 2]);
        ready(&mut first.promise).unwrap();
        ready(&mut second.promise).unwrap();
        drop((first, second, client));
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn bounded_local_admission_preserves_order_and_large_results() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = seen.clone();
        let bytes = if cfg!(miri) { 257 } else { 65536 };
        let client = local(
            &mut driver,
            &mut hooks,
            server(move |method, _, mut results| {
                recorded.borrow_mut().push(method);
                results
                    .get()
                    .set_as::<capnp::data::Owned>(vec![method as u8; bytes].as_slice())
                    .unwrap();
                ok()
            }),
        );
        let mut first = request(&client, 1).send();
        let mut second = request(&client, 2).send();
        let mut calls = Calls::new(1, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        assert_eq!(*seen.borrow(), [1, 2]);
        for (remote, byte) in [(&mut first, 1), (&mut second, 2)] {
            let response = ready(&mut remote.promise).unwrap();
            assert_eq!(
                response
                    .get()
                    .unwrap()
                    .get_as::<capnp::data::Reader>()
                    .unwrap(),
                vec![byte; bytes]
            );
        }
        drop((first, second, client));
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        assert!(calls.is_empty());
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn early_local_pipeline_survives_parent_failure_and_late_projection() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let child = capability::Client::new(
            server(|_, _, mut results| {
                results
                    .get()
                    .set_as::<capnp::text::Owned>("child result")
                    .unwrap();
                ok()
            })
            .add_ref(),
        );
        let (finish, done) = shared::channel::<()>();
        let done = Rc::new(RefCell::new(Some(done)));
        let client = local(
            &mut driver,
            &mut hooks,
            server(move |_, _, mut results| {
                results.get().set_as_capability(child.hook.add_ref());
                results.set_pipeline().unwrap();
                let done = done.borrow_mut().take().unwrap();
                capability::DispatchCallResult {
                    promise: Promise::from_future(async move {
                        done.await.unwrap();
                        drop(results);
                        let mut error = Error::overloaded("parent detail".into());
                        error.set_detail(42, vec![1, 2, 3]);
                        Err(error)
                    }),
                    is_streaming: false,
                    allow_cancellation: true,
                }
            }),
        );
        let mut parent = request(&client, 1).send();
        let piped = capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
        let mut child_call = request(&piped, 2).send();
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        assert_eq!(
            ready(&mut child_call.promise)
                .unwrap()
                .get()
                .unwrap()
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "child result"
        );
        assert!(parent
            .promise
            .poll_unpin(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        finish.send(()).unwrap();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        let error = match ready(&mut parent.promise) {
            Err(e) => e,
            Ok(_) => panic!("parent should fail"),
        };
        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(error.extra, "parent detail");
        assert_eq!(
            error.details().collect::<Vec<_>>(),
            vec![(42, &[1, 2, 3][..])]
        );
        let late = capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
        let mut late_call = request(&late, 3).send();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        assert_eq!(
            ready(&mut late_call.promise)
                .unwrap()
                .get()
                .unwrap()
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "child result"
        );
        drop((parent, child_call, late_call, piped, late, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

struct CountWake(AtomicUsize);
impl std::task::Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn pending_methods_register_wakes_without_starving_ready_work_or_spinning() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let observed = Rc::new(RefCell::new(Vec::new()));
        let registrations = observed.clone();
        let client = local(
            &mut driver,
            &mut hooks,
            server(move |method, _, mut results| {
                if method == 3 {
                    results.get().set_as::<capnp::text::Owned>("ready").unwrap();
                    return ok();
                }
                let registrations = registrations.clone();
                capability::DispatchCallResult {
                    promise: Promise::from_future(futures::future::poll_fn(move |cx| {
                        if registrations.borrow().len() < usize::from(method) {
                            registrations.borrow_mut().push(cx.waker().clone());
                        }
                        let _ = &results;
                        Poll::Pending
                    })),
                    is_streaming: false,
                    allow_cancellation: true,
                }
            }),
        );
        let first = request(&client, 1).send();
        let second = request(&client, 2).send();
        let mut third = request(&client, 3).send();
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        for _ in 0..256 {
            let _ = tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut cx,
            );
        }
        assert_eq!(observed.borrow().len(), 2);
        ready(&mut third.promise).unwrap();
        // A whole quiescent scan stops self-waking; partial scans remain bounded.
        let mut quiescent = false;
        for _ in 0..128 {
            let before = counter.0.load(Ordering::SeqCst);
            let state = tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut cx,
            );
            if state.is_pending() && counter.0.load(Ordering::SeqCst) == before {
                quiescent = true;
                break;
            }
        }
        assert!(quiescent, "quiescent calls must stop self-waking");
        observed.borrow()[0].wake_by_ref();
        let before = counter.0.load(Ordering::SeqCst);
        assert!(before > 0);
        drop((first, second, third, client));
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        assert!(calls.is_empty());
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[derive(Default)]
struct Executor(RefCell<Vec<Promise<(), Error>>>);
impl capability::CallExecutor for Executor {
    fn spawn(&self, task: Promise<(), Error>) -> capnp::Result<()> {
        self.0.borrow_mut().push(task);
        Ok(())
    }
}
#[test]
fn cancellation_keeps_protected_execution_and_reclaims_local_bindings() {
    for (early, cancellable) in [(true, true), (false, true), (false, false)] {
        with_peer(|mut driver, messages| {
            let entered = Rc::new(Cell::new(false));
            let done = Rc::new(Cell::new(false));
            let entered2 = entered.clone();
            let done2 = done.clone();
            let (finish, receiver) = shared::channel();
            let receiver = Rc::new(RefCell::new(Some(receiver)));
            let mut hooks = ClientHooks::new(24, 24);
            let client = local(
                &mut driver,
                &mut hooks,
                server(move |_, _, mut results| {
                    entered2.set(true);
                    results
                        .get()
                        .set_as::<capnp::text::Owned>("partial")
                        .unwrap();
                    let receiver = receiver.borrow_mut().take().unwrap();
                    let done = done2.clone();
                    capability::DispatchCallResult::with_cancellation_policy(
                        Promise::from_future(async move {
                            receiver.await.map_err(crate::canceled_to_error)?;
                            done.set(true);
                            drop(results);
                            Ok(())
                        }),
                        false,
                        cancellable,
                    )
                }),
            );
            let call = request(&client, 1).send();
            let executor = Rc::new(Executor::default());
            let mut calls = Calls::new(8, Some(executor.clone())).unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            if !early {
                drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
                assert!(entered.get());
            }
            drop(call);
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            assert!(calls.is_empty());
            assert_eq!(entered.get(), !early);
            if !early && !cancellable {
                finish.send(()).unwrap();
                executor
                    .0
                    .borrow_mut()
                    .pop()
                    .unwrap()
                    .now_or_never()
                    .unwrap()
                    .unwrap();
                assert!(done.get());
            } else {
                assert!(!done.get());
                assert!(executor.0.borrow().is_empty());
            }
            drop(client);
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn independently_polled_early_pipeline_wakes_the_coordinator() {
    with_peer(|mut driver, messages| {
        let executor = Rc::new(Executor::default());
        let mut hooks = ClientHooks::new(24, 24);
        let child = server(|_, _, _| ok());
        let (publish, gate) = shared::channel::<()>();
        let gate = Rc::new(RefCell::new(Some(gate)));
        let client = local(
            &mut driver,
            &mut hooks,
            server(move |_, _, mut results| {
                let child = child.clone();
                let gate = gate.borrow_mut().take().unwrap();
                capability::DispatchCallResult::with_cancellation_policy(
                    Promise::from_future(async move {
                        gate.await.map_err(crate::canceled_to_error)?;
                        results.get().set_as_capability(child.add_ref());
                        results.set_pipeline()?;
                        futures::future::pending::<()>().await;
                        Ok(())
                    }),
                    false,
                    false,
                )
            }),
        );
        let parent = request(&client, 1).send();
        let mut calls = Calls::new(8, Some(executor.clone())).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        let counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        for _ in 0..96 {
            let _ = tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut cx,
            );
        }
        let mut task = executor.0.borrow_mut().pop().expect("protected task");
        publish.send(()).unwrap();
        let before = counter.0.load(Ordering::SeqCst);
        assert!(task
            .poll_unpin(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(
            counter.0.load(Ordering::SeqCst) > before,
            "early publication must wake the coordinator independently of method completion"
        );
        let piped = capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
        let mut child_call = request(&piped, 2).send();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        ready(&mut child_call.promise).unwrap();
        drop((task, parent, child_call, piped, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[derive(Default)]
struct Sink {
    bytes: Vec<u8>,
    polls: usize,
}
impl capntproto_rpc_engine::io::Flush for Sink {
    type Error = ();
    fn poll_flush(
        &mut self,
        _: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), capntproto_rpc_engine::io::IoError<Self::Error>>> {
        core::task::Poll::Ready(Ok(()))
    }
}
impl io::Write for Sink {
    fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, io::IoError<()>>> {
        self.polls += 1;
        if self.polls.is_multiple_of(5) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if self.polls.is_multiple_of(7) {
            return Poll::Ready(Err(io::IoError::Interrupted));
        }
        let n = bytes.len().min(31);
        self.bytes.extend_from_slice(&bytes[..n]);
        Poll::Ready(Ok(n))
    }
}
fn drain<'p>(
    queue: &mut messages::Queue<'p, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
) -> Vec<u8> {
    let mut sink = Sink::default();
    for _ in 0..100_000 {
        if let Poll::Ready(result) = io::poll_queue(
            &mut sink,
            queue,
            messages,
            &mut Context::from_waker(Waker::noop()),
            NonZeroUsize::new(3).unwrap(),
        ) {
            result.unwrap();
            return sink.bytes;
        }
    }
    panic!("drain");
}
#[test]
fn large_remote_call_retries_output_and_preserves_exception_diagnostics() {
    for no_pipeline in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            let size = if cfg!(miri) { 257 } else { 65536 };
            let data = vec![0xa5; size];
            let mut req = request(&client, 1);
            req.hook.set_hints(CallHints {
                no_promise_pipelining: no_pipeline,
                ..Default::default()
            });
            req.get()
                .set_as::<capnp::data::Owned>(data.as_slice())
                .unwrap();
            let mut response = req.send();
            let mut calls = Calls::new(8, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let writer = messages.reserve(16).unwrap();
            messages.write(&writer)[..16].fill(0);
            messages.write(&writer)[4] = 1;
            let frame = messages.seal(writer, 16).unwrap_or_else(|_| panic!());
            let offer = messages.offer(&frame).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(frame);
            let mut rejected = 0;
            for _ in 0..64 {
                if matches!(
                    tick(
                        &mut calls,
                        &mut hooks,
                        &mut driver,
                        messages,
                        &mut queue,
                        &mut Context::from_waker(Waker::noop())
                    ),
                    Poll::Ready(Err(_))
                ) {
                    rejected += 1;
                }
            }
            assert!(rejected > 0);
            assert_eq!(
                calls.entries.iter().flatten().count() + usize::from(calls.pending.is_some()),
                1
            );
            assert_eq!(
                calls.invocations.capacity(),
                0,
                "settled remote calls need no completion channel"
            );
            if no_pipeline {
                assert!(
                    calls.pending.is_some(),
                    "failed immediate output preserves the request"
                );
            }
            assert_eq!(drain(&mut queue, messages).len(), 16);
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            let bytes = drain(&mut queue, messages);
            let mut remaining = bytes.as_slice();
            let decoded =
                capnp::serialize::read_message(&mut remaining, Default::default()).unwrap();
            assert!(remaining.is_empty(), "retries must not duplicate the Call");
            let rpc_capnp::message::Call(call) = decoded
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!();
            };
            let call = call.unwrap();
            assert_eq!(
                call.get_params()
                    .unwrap()
                    .get_content()
                    .get_as::<capnp::data::Reader>()
                    .unwrap(),
                data
            );
            let frame = Frame::build(messages, 512, Default::default(), |b| {
                calls::exception(
                    b,
                    capntproto_rpc_engine::ids::AnswerId::from_wire(call.get_question_id()),
                    capnp::ErrorKind::Overloaded,
                    "peer overloaded",
                )
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [0; 24], |_| (), || ())
                .unwrap_or_else(|_| panic!("receive"));
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            let error = match ready(&mut response.promise) {
                Err(error) => error,
                Ok(_) => panic!("exception"),
            };
            assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
            assert_eq!(error.extra, "remote exception: peer overloaded");
            drop((response, client));
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn publication_resolves_waiting_children_before_the_parent_flushes() {
    for no_pipeline in [false, true] {
        with_peer(|mut driver, messages| {
            driver.use_owned_output_queues(true);
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            let parent = request(&client, 1).send();
            let child = capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
            let mut child_request = request(&child, 2);
            child_request.hook.set_hints(capability::CallHints {
                no_promise_pipelining: no_pipeline,
                ..Default::default()
            });
            let child_reply = child_request.send();
            let mut calls = Calls::new(8, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 8).unwrap();
            assert_eq!(hooks.inbox.queue.borrow().len(), 1);
            let progress = tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut Context::from_waker(Waker::noop()),
            );
            assert!(matches!(progress, Poll::Ready(Ok(Progress::Published))));
            assert_eq!(queue.len(), 1, "the parent has not been flushed");
            assert_eq!(
                hooks.inbox.queue.borrow().len(),
                1,
                "publication must route the already-waiting child"
            );
            drop(parent);
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            assert_eq!(queue.len(), 2, "both calls can share one transport batch");
            assert!(
                calls.entries[0]
                    .as_mut()
                    .unwrap()
                    .wire
                    .poll_canceled(&mut Context::from_waker(Waker::noop()))
                    .is_pending(),
                "the adopted child retains the parent after its public promise is dropped"
            );
            let bytes = drain(&mut queue, messages);
            let mut remaining = bytes.as_slice();
            let first = capnp::serialize::read_message(&mut remaining, Default::default()).unwrap();
            let rpc_capnp::message::Call(first) = first
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("parent Call")
            };
            let first = first.unwrap();
            assert_eq!(first.get_method_id(), 1);
            let second =
                capnp::serialize::read_message(&mut remaining, Default::default()).unwrap();
            let rpc_capnp::message::Call(second) = second
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("child Call")
            };
            let second = second.unwrap();
            assert_eq!(second.get_method_id(), 2);
            let rpc_capnp::message_target::PromisedAnswer(target) =
                second.get_target().unwrap().which().unwrap()
            else {
                panic!("promised answer")
            };
            assert_eq!(target.unwrap().get_question_id(), first.get_question_id());
            assert!(remaining.is_empty());
            drop((child_reply, child, client));
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn direct_remote_projections_keep_the_parent_question_before_and_after_publication() {
    for project_before_send in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            let older = project_before_send.then(|| {
                let target = driver.runtime.promise(&mut None).unwrap();
                let task = driver
                    .enqueue_dynamic(
                        &target,
                        DynamicCall::default(),
                        &[],
                        messages,
                        128,
                        Default::default(),
                        |_| Ok(()),
                    )
                    .unwrap();
                (target, task)
            });
            let mut parent = request(&client, 1).send();
            let mut calls = Calls::new(8, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 8).unwrap();
            let Poll::Ready(Ok(progress)) = tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut Context::from_waker(Waker::noop()),
            ) else {
                panic!("admit parent")
            };
            assert_eq!(
                progress,
                if project_before_send {
                    Progress::Advanced
                } else {
                    Progress::Published
                }
            );
            if !project_before_send {
                drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            }
            let child = capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
            if project_before_send {
                calls.entries[0]
                    .as_mut()
                    .unwrap()
                    .wire
                    .poll_pipelines(&mut hooks, &mut driver, &mut calls.invocations, messages)
                    .unwrap();
            } else {
                drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            }
            assert_eq!(calls.invocations.capacity(), 0);
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            let Some(DirectStage::Sent(question)) = calls.entries[0].as_ref().unwrap().wire.direct
            else {
                panic!("published direct parent")
            };
            let parent_bytes = drain(&mut queue, messages);
            let decoded =
                capnp::serialize::read_message(parent_bytes.as_slice(), Default::default())
                    .unwrap();
            let rpc_capnp::message::Call(parent_call) = decoded
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("parent Call")
            };
            assert!(!parent_call.unwrap().get_no_promise_pipelining());
            for _ in 0..64 {
                driver
                    .poll_maintenance(
                        messages,
                        &mut queue,
                        128,
                        Default::default(),
                        &mut [0; 24],
                        NonZeroUsize::new(8).unwrap(),
                    )
                    .unwrap();
                hooks.collect(&mut driver.runtime);
            }
            let child_reply = request(&child, 2).send();
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            if !project_before_send {
                assert!(calls
                    .entries
                    .iter()
                    .flatten()
                    .all(|entry| entry.wire.task.is_none()));
            }
            let bytes = drain(&mut queue, messages);
            let mut remaining = bytes.as_slice();
            let decoded =
                capnp::serialize::read_message(&mut remaining, Default::default()).unwrap();
            assert!(remaining.is_empty());
            let rpc_capnp::message::Call(call) = decoded
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("child Call")
            };
            let call = call.unwrap();
            assert_eq!(call.get_method_id(), 2);
            let rpc_capnp::message_target::PromisedAnswer(target) =
                call.get_target().unwrap().which().unwrap()
            else {
                panic!("promised-answer route")
            };
            assert_eq!(target.unwrap().get_question_id(), question.id().to_wire());
            let reply = Frame::build(messages, 512, Default::default(), |builder| {
                calls::exception(
                    builder,
                    capntproto_rpc_engine::ids::AnswerId::from_wire(question.id().to_wire()),
                    capnp::ErrorKind::Overloaded,
                    "parent rejected",
                )
            })
            .unwrap();
            driver
                .receive(reply, messages, &mut [0; 24], |_| (), || ())
                .unwrap_or_else(|_| panic!("parent Return"));
            // Return must propagate to existing wire projections before the
            // parent result owner can release its question.
            for _ in 0..64 {
                driver
                    .poll_maintenance(
                        messages,
                        &mut queue,
                        128,
                        Default::default(),
                        &mut [0; 24],
                        NonZeroUsize::new(8).unwrap(),
                    )
                    .unwrap();
                hooks.collect(&mut driver.runtime);
                drain(&mut queue, messages);
            }
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            let error = ready(&mut parent.promise).err().expect("parent exception");
            assert_eq!(error.extra, "remote exception: parent rejected");
            let late = capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
            let mut late_reply = request(&late, 3).send();
            let error = ready(&mut late_reply.promise)
                .err()
                .expect("late projection exception");
            assert_eq!(error.extra, "remote exception: parent rejected");
            drop((late_reply, late, child_reply, child, parent, client));
            if let Some((target, task)) = older {
                assert!(driver.cancel_invocation(task, messages));
                driver.runtime.release(target);
            }
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn queued_pipeline_sink_failure_reclaims_its_unpublished_task() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let client = remote(&mut driver, &mut hooks);
        let older = driver.runtime.promise(&mut None).unwrap();
        let task = driver
            .enqueue_dynamic(
                &older,
                DynamicCall::default(),
                &[],
                messages,
                128,
                Default::default(),
                |_| Ok(()),
            )
            .unwrap();
        let response = request(&client, 1).send();
        hooks
            .inbox
            .queue
            .borrow_mut()
            .front_mut()
            .unwrap()
            .results
            .hook()
            .set_pipeline_from(Box::new(crate::broken::Pipeline::new(Error::failed(
                "earlier pipeline".into(),
            ))))
            .unwrap();
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let Poll::Ready(Err(error)) = tick(
            &mut calls,
            &mut hooks,
            &mut driver,
            messages,
            &mut queue,
            &mut Context::from_waker(Waker::noop()),
        ) else {
            panic!("pipeline sink rejection")
        };
        assert_eq!(error.extra, "set_pipeline() called twice");
        assert_eq!(driver.runtime.queued(), 1, "only the older task remains");
        assert!(driver.runtime.next_ready().unwrap().is_none());
        assert!(queue.is_empty());
        assert_eq!(calls.invocations.capacity(), 0);
        assert!(driver.cancel_invocation(task, messages));
        driver.runtime.release(older);
        drop((response, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn immediate_pipeline_sink_failure_cancels_the_published_question() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let client = remote(&mut driver, &mut hooks);
        let mut response = request(&client, 1).send();
        // An independently supplied early pipeline makes this sink reject a
        // second publication. The accepted wire Call must still be flushed and
        // retained for late-Return cleanup, without leaking a hosted owner.
        hooks
            .inbox
            .queue
            .borrow_mut()
            .front_mut()
            .unwrap()
            .results
            .hook()
            .set_pipeline_from(Box::new(crate::broken::Pipeline::new(Error::failed(
                "earlier pipeline".into(),
            ))))
            .unwrap();
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        assert!(matches!(
            tick(
                &mut calls,
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                &mut Context::from_waker(Waker::noop())
            ),
            Poll::Ready(Ok(Progress::Published))
        ));
        assert!(calls.is_empty());
        let error = ready(&mut response.promise)
            .err()
            .expect("pipeline sink failure");
        assert_eq!(error.extra, "set_pipeline() called twice");
        let bytes = drain(&mut queue, messages);
        let message = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
        let rpc_capnp::message::Call(call) = message
            .get_root::<rpc_capnp::message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!("published Call")
        };
        let id = call.unwrap().get_question_id();
        let frame = Frame::build(messages, 128, Default::default(), |builder| {
            calls::acknowledge(builder, capntproto_rpc_engine::ids::AnswerId::from_wire(id))
        })
        .unwrap();
        driver
            .receive(frame, messages, &mut [0; 24], |_| (), || ())
            .unwrap_or_else(|_| panic!("late Return"));
        for _ in 0..128 {
            driver
                .poll_maintenance(
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    &mut [0; 24],
                    NonZeroUsize::new(8).unwrap(),
                )
                .unwrap();
            drain(&mut queue, messages);
        }
        assert!(driver.ledgers.questions.is_empty());
        drop((response, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn direct_remote_cancellation_reclaims_queued_and_late_return_owners() {
    for (sent, no_pipeline) in [(false, false), (false, true), (true, false), (true, true)] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            // An older unresolved task forces the ordinary dispatch path. The
            // ready remote call may pass it once enqueued, but cannot skip the
            // immediate-publication ordering preflight.
            let unresolved = driver.runtime.promise(&mut None).unwrap();
            let queued = driver
                .enqueue_dynamic(
                    &unresolved,
                    DynamicCall::default(),
                    &[],
                    messages,
                    128,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            let mut request = request(&client, 1);
            request.hook.set_hints(CallHints {
                no_promise_pipelining: no_pipeline,
                ..Default::default()
            });
            let response = request.send();
            let mut calls = Calls::new(8, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            assert!(matches!(
                tick(
                    &mut calls,
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut queue,
                    &mut Context::from_waker(Waker::noop())
                ),
                Poll::Ready(Ok(Progress::Advanced))
            ));
            assert!(matches!(
                calls.entries[0].as_ref().unwrap().wire.direct,
                Some(DirectStage::Queued)
            ));
            let question = if sent {
                drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
                let Some(DirectStage::Sent(question)) =
                    calls.entries[0].as_ref().unwrap().wire.direct
                else {
                    panic!()
                };
                assert!(!drain(&mut queue, messages).is_empty());
                Some(question)
            } else {
                None
            };
            drop(response);
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            assert!(calls.is_empty());
            assert!(driver.runtime.next_ready().unwrap().is_none());
            assert_eq!(calls.invocations.capacity(), 0);
            assert!(driver.cancel_invocation(queued, messages));
            driver.runtime.release(unresolved);
            if let Some(question) = question {
                let reply = Frame::build(messages, 512, Default::default(), |builder| {
                    calls::acknowledge(
                        builder,
                        capntproto_rpc_engine::ids::AnswerId::from_wire(question.id().to_wire()),
                    )
                })
                .unwrap();
                driver
                    .receive(reply, messages, &mut [0; 24], |_| (), || ())
                    .unwrap_or_else(|rejected| panic!("late Return: {:?}", rejected.error));
                for _ in 0..128 {
                    driver
                        .poll_maintenance(
                            messages,
                            &mut queue,
                            128,
                            Default::default(),
                            &mut [0; 24],
                            NonZeroUsize::new(1).unwrap(),
                        )
                        .unwrap();
                    drain(&mut queue, messages);
                }
                assert!(driver.ledgers.questions.is_empty());
            } else {
                assert!(queue.is_empty());
            }
            drop(client);
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn burst_admission_keeps_older_cancellation_fair_when_capacity_fills() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let client = remote(&mut driver, &mut hooks);
        let mut calls = Calls::new(16, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut responses = Vec::new();
        for index in 0..24 {
            let mut request = request(&client, 1);
            request.hook.set_hints(CallHints {
                no_promise_pipelining: true,
                ..Default::default()
            });
            responses.push(Some(request.send()));
            if index < 9 {
                drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
                drain(&mut queue, messages);
            }
        }
        let Some(DirectStage::Sent(first)) = calls.entries[0].as_ref().unwrap().wire.direct else {
            panic!("direct call")
        };
        drop(responses[0].take());
        let mut retired = false;
        for _ in 0..64 {
            if let Poll::Ready(result) = calls.poll(
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                128,
                Default::default(),
                &mut [0; 24],
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(8).unwrap(),
                import,
            ) {
                result.unwrap();
            }
            drain(&mut queue, messages);
            if calls.immediate_receiver(first).is_none() {
                retired = true;
                break;
            }
        }
        assert!(
            retired,
            "new admissions must not starve an older cancellation"
        );
        assert!(
            !hooks.inbox.queue.borrow().is_empty(),
            "exercise cancellation while newer requests still wait"
        );
        drop((responses, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn immediate_reply_index_survives_bursts_cancellation_and_question_reuse() {
    for budget in [1, 8] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            let mut calls = Calls::new(16, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let mut previous = Vec::new();
            for round in 0..3 {
                let mut responses = Vec::new();
                for _ in 0..12 {
                    let mut request = request(&client, 1);
                    request.hook.set_hints(CallHints {
                        no_promise_pipelining: true,
                        ..Default::default()
                    });
                    responses.push(Some(request.send()));
                    let mut published = false;
                    for _ in 0..256 {
                        if let Poll::Ready(result) = calls.poll(
                            &mut hooks,
                            &mut driver,
                            messages,
                            &mut queue,
                            128,
                            Default::default(),
                            &mut [0; 24],
                            &mut Context::from_waker(Waker::noop()),
                            NonZeroUsize::new(budget).unwrap(),
                            import,
                        ) {
                            if result.unwrap() == Progress::Published {
                                published = true;
                                break;
                            }
                        }
                    }
                    assert!(published);
                    drain(&mut queue, messages);
                }
                let questions: Vec<_> = calls
                    .entries
                    .iter()
                    .flatten()
                    .map(|entry| {
                        let Some(DirectStage::Sent(key)) = entry.wire.direct else {
                            panic!("immediate call")
                        };
                        key
                    })
                    .collect();
                assert_eq!(calls.reply_owners.len(), 12);
                for old in &previous {
                    assert!(
                        calls.plain_receiver(*old).is_none(),
                        "stale generation selected a new owner"
                    );
                    assert!(calls.immediate_receiver(*old).is_none());
                }
                if round != 0 {
                    assert!(
                        previous
                            .iter()
                            .any(|old| questions.iter().any(|key| old.id() == key.id())),
                        "exercise question ID reuse"
                    );
                    drop(responses[1].take());
                    assert!(calls.plain_receiver(questions[1]).is_none());
                }
                // Replies arrive in reverse order, with a canceled reply mixed in.
                for index in (0..12).rev() {
                    let key = questions[index];
                    let reply = Frame::build(messages, 128, Default::default(), |builder| {
                        use rpc_capnp::api::{Message, Return};
                        let root = builder.try_init_root::<Message>()?;
                        let mut result = root.into_field(Message::RETURN).init()?;
                        result.scalar(Return::ANSWER_ID).set(key.id().to_wire());
                        result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                        result.scalar(Return::NO_FINISH_NEEDED).set(true);
                        result.into_field(Return::RESULTS).init()?;
                        Ok(())
                    })
                    .unwrap();
                    if round == 0 && index % 2 == 0 {
                        // An owned return admitted through the general reader uses
                        // the same indexed owner and full generation check.
                        match driver.receive(reply, messages, &mut [0; 24], |_| (), || ()) {
                            Ok(connection::Received::Returned(returned)) => {
                                assert_eq!(returned, key)
                            }
                            _ => panic!("ordinary return admission"),
                        }
                        assert_eq!(
                            calls
                                .returned(
                                    key,
                                    &mut hooks,
                                    &mut driver,
                                    messages,
                                    &mut Context::from_waker(Waker::noop()),
                                    |hooks, driver| { hooks.collect(&mut driver.runtime) }
                                )
                                .unwrap(),
                            Some(true)
                        );
                        let mut response = responses[index].take().unwrap();
                        ready(&mut response.promise).unwrap();
                        continue;
                    }
                    let received = match driver
                        .try_receive_plain_owned(reply, messages, |key| calls.plain_receiver(key))
                    {
                        Ok(received) => received,
                        Err((error, _)) => panic!("plain return: {error:?}"),
                    };
                    match received {
                        connection::PlainReceived::Delivered(owner, message) => {
                            assert!(responses[index].is_some());
                            calls.deliver_plain(owner, message);
                            assert!(calls.plain_receiver(key).is_none());
                            let mut response = responses[index].take().unwrap();
                            ready(&mut response.promise).unwrap();
                        }
                        connection::PlainReceived::Deferred(frame) => {
                            assert!(
                                responses[index].is_none(),
                                "live immediate reply missed its index"
                            );
                            if let Err(rejected) =
                                driver.receive(frame, messages, &mut [0; 24], |_| (), || ())
                            {
                                panic!("canceled return: {:?}", rejected.error);
                            }
                            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
                        }
                    }
                }
                assert!(calls.is_empty());
                assert!(
                    calls.reply_owners.is_empty(),
                    "retirement must release every index entry"
                );
                previous = questions;
                drop(responses);
            }
            drop(client);
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn immediate_calls_deliver_out_of_order_and_drain_a_canceled_reply() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let client = remote(&mut driver, &mut hooks);
        let mut responses = Vec::new();
        for _ in 0..4 {
            let mut request = request(&client, 1);
            request.hook.set_hints(CallHints {
                no_promise_pipelining: true,
                ..Default::default()
            });
            responses.push(Some(request.send()));
        }
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        fn poll<'p, 'id>(
            calls: &mut TestCalls<'_, '_, 'p, 'id>,
            hooks: &mut ClientHooks<'id>,
            driver: &mut TestPeer<'p, 'id, '_>,
            messages: &mut messages::Messages<'p, '_, '_>,
            queue: &mut messages::Queue<'p, '_>,
        ) -> Poll<capnp::Result<Progress>> {
            calls.poll(
                hooks,
                driver,
                messages,
                queue,
                128,
                Default::default(),
                &mut [0; 24],
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(8).unwrap(),
                import,
            )
        }
        for _ in 0..4 {
            assert!(matches!(
                poll(&mut calls, &mut hooks, &mut driver, messages, &mut queue),
                Poll::Ready(Ok(Progress::Published))
            ));
        }
        assert_eq!(calls.invocations.capacity(), 0);
        let questions = calls
            .entries
            .iter()
            .flatten()
            .map(|entry| {
                assert!(entry.wire.task.is_none());
                let Some(DirectStage::Sent(question)) = entry.wire.direct else {
                    panic!()
                };
                question
            })
            .collect::<Vec<_>>();
        drain(&mut queue, messages);
        drop(responses[1].take());
        // Complete the last call while earlier replies are still missing.
        for index in [3, 1, 2, 0] {
            let reply = Frame::build(messages, 128, Default::default(), |builder| {
                calls::acknowledge(
                    builder,
                    capntproto_rpc_engine::ids::AnswerId::from_wire(
                        questions[index].id().to_wire(),
                    ),
                )
            })
            .unwrap();
            driver
                .receive(reply, messages, &mut [0; 24], |_| (), || ())
                .unwrap_or_else(|rejected| panic!("Return: {:?}", rejected.error));
            for _ in 0..8 {
                if let Poll::Ready(Err(error)) =
                    poll(&mut calls, &mut hooks, &mut driver, messages, &mut queue)
                {
                    panic!("{error}");
                }
            }
            if let Some(mut response) = responses[index].take() {
                ready(&mut response.promise).unwrap();
            }
        }
        assert!(calls.is_empty());
        drop((responses, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn disconnect_prevents_admission_and_termination_recovers_running_requests() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let entered = Rc::new(Cell::new(0));
        let counted = entered.clone();
        let client = local(
            &mut driver,
            &mut hooks,
            server(move |_, _, _| {
                counted.set(counted.get() + 1);
                ok()
            }),
        );
        let response = request(&client, 1).send();
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        driver.disconnect(capnp::ErrorKind::Disconnected);
        assert!(
            matches!(tick(&mut calls, &mut hooks, &mut driver, messages, &mut queue, &mut Context::from_waker(Waker::noop())), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected)
        );
        assert_eq!(entered.get(), 0);
        drop((response, client));
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn immediate_single_capability_calls_keep_only_reachable_grants() {
    for reachable in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            let cap = capability::Client::new(server(|_, _, _| ok()).add_ref());
            let mut req = request(&client, 1);
            req.get().set_as_capability(cap.hook.add_ref());
            if !reachable {
                req.get().set_as::<capnp::text::Owned>("data only").unwrap();
            }
            let response = req.send();
            let mut calls = Calls::new(8, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            for _ in 0..128 {
                if let Poll::Ready(result) = tick(
                    &mut calls,
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut queue,
                    &mut Context::from_waker(Waker::noop()),
                ) {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            assert_eq!(queue.len(), 1);
            let entry = calls.entries.iter().flatten().next().unwrap();
            assert!(
                entry.wire.task.is_none(),
                "settled publication creates no temporary queued task"
            );
            assert!(matches!(entry.wire.direct, Some(DirectStage::Sent(_))));
            let bytes = drain(&mut queue, messages);
            let reader =
                capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
            let root = reader.get_root::<rpc_capnp::message::Reader>().unwrap();
            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                panic!("Call")
            };
            let params = call.unwrap().get_params().unwrap();
            assert_eq!(params.get_cap_table().unwrap().len(), u32::from(reachable));
            if reachable {
                assert_eq!(params.get_content().capability_index().unwrap(), 0);
            } else {
                assert_eq!(
                    params
                        .get_content()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "data only"
                );
            }
            drop((response, client, cap));
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn resource_output_retries_publication_and_attaches_only_to_first_write() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let client = remote(&mut driver, &mut hooks);
        let cap = capability::Client::new(server(|_, _, _| ok()).add_ref());
        let mut req = request(&client, 1);
        req.get().set_as_capability(cap.hook.add_ref());
        let response = req.send();
        let mut calls = Calls::new(8, None).unwrap();
        let mut queue =
            messages::Queue::<capntproto_rpc_wire::attachments::Sender<u32, 2>>::growing_with_resources(0, 4)
                .unwrap();
        let mut rejected = false;
        for _ in 0..96 {
            if matches!(
                calls.poll_with_attachments(
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    &mut [],
                    &mut Context::from_waker(Waker::noop()),
                    NonZeroUsize::new(1).unwrap(),
                    capntproto_rpc_wire::Capacity::Inline,
                    |_| None,
                    |_| Some(91),
                    import
                ),
                Poll::Ready(Err(_))
            ) {
                rejected = true;
                break;
            }
        }
        assert!(rejected);
        assert!(queue.is_empty());
        for _ in 0..96 {
            if let Poll::Ready(Err(e)) = calls.poll_with_attachments(
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                128,
                Default::default(),
                &mut [0; 24],
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(1).unwrap(),
                capntproto_rpc_wire::Capacity::Inline,
                |_| None,
                |_| Some(91),
                import,
            ) {
                panic!("{e}");
            }
        }
        struct Output {
            sink: Sink,
            attached: Vec<u32>,
        }
        impl capntproto_rpc_engine::io::Flush for Output {
            type Error = ();
            fn poll_flush(
                &mut self,
                _: &mut core::task::Context<'_>,
            ) -> core::task::Poll<Result<(), capntproto_rpc_engine::io::IoError<Self::Error>>>
            {
                core::task::Poll::Ready(Ok(()))
            }
        }
        impl io::WriteResources<capntproto_rpc_wire::attachments::Sender<u32, 2>> for Output {
            fn poll_write_resources(
                &mut self,
                cx: &mut Context<'_>,
                bytes: &[u8],
                resources: Option<&capntproto_rpc_wire::attachments::Sender<u32, 2>>,
            ) -> Poll<Result<usize, io::IoError<()>>> {
                let result = io::Write::poll_write(&mut self.sink, cx, bytes);
                if matches!(result, Poll::Ready(Ok(n)) if n != 0) {
                    if let Some(resources) = resources {
                        self.attached.extend(resources.iter().copied());
                    }
                }
                result
            }
        }
        let mut output = Output {
            sink: Sink {
                polls: 4,
                ..Default::default()
            },
            attached: Vec::new(),
        };
        for _ in 0..1024 {
            if let Poll::Ready(result) = io::poll_queue_resources(
                &mut output,
                &mut queue,
                messages,
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(1).unwrap(),
            ) {
                result.unwrap();
                break;
            }
        }
        assert!(queue.is_empty());
        assert_eq!(output.attached, [91]);
        let mut bytes = output.sink.bytes.as_slice();
        let message = capnp::serialize::read_message(&mut bytes, Default::default()).unwrap();
        assert!(bytes.is_empty());
        let rpc_capnp::message::Call(call) = message
            .get_root::<rpc_capnp::message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!("Call");
        };
        assert_eq!(
            call.unwrap()
                .get_params()
                .unwrap()
                .get_cap_table()
                .unwrap()
                .get(0)
                .get_attached_fd(),
            0
        );
        drop((response, client, cap));
        calls.terminate(&mut driver, messages);
        hooks.close(&mut driver.runtime, Error::disconnected("close".into()));
        let mut closing = driver.terminate(&mut queue);
        let mut done = false;
        for _ in 0..1024 {
            if matches!(
                closing
                    .poll(messages, NonZeroUsize::new(32).unwrap())
                    .unwrap(),
                connection::Termination::Done
            ) {
                done = true;
                break;
            }
        }
        assert!(done);
        let writers: Vec<_> = (0..32).map(|_| messages.reserve(128).unwrap()).collect();
        for writer in writers {
            messages.discard(writer);
        }
    });
}

#[test]
fn foreign_failed_promise_completes_calls_with_original_diagnostics() {
    use crate::scoped::wire::foreign::{self, ForeignPromises};
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let mut foreign = ForeignPromises::new(4);
        let mut error = Error::overloaded("original foreign exception".into());
        error.set_detail(37, vec![8, 9]);
        let failed: capability::Client = crate::new_future_client(Promise::err(error.clone()));
        let route = foreign.import(&mut driver.runtime, &*failed.hook).unwrap();
        let client = hooks
            .export_client_with_metadata(&mut driver.runtime, &route, foreign::metadata)
            .unwrap();
        // Queue before metadata reports failure, then admit after graph
        // settlement. This exercises the retained failed endpoint owner.
        let mut response = request(&client, 1).send();
        for _ in 0..16 {
            if let Poll::Ready(result) = foreign.poll(
                &mut hooks,
                &mut driver.runtime,
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(1).unwrap(),
            ) {
                result.unwrap();
            }
            if foreign.is_empty() {
                break;
            }
        }
        assert!(foreign.is_empty());
        let observed = ready(&mut client.hook.when_resolved()).unwrap_err();
        assert_eq!(observed.extra, error.extra);
        assert_eq!(observed.detail(37), error.detail(37));
        let mut calls = Calls::new(4, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        let failure = match ready(&mut response.promise) {
            Err(e) => e,
            Ok(_) => panic!("failed promise accepted a call"),
        };
        assert_eq!(failure.kind, error.kind);
        assert_eq!(failure.extra, error.extra);
        assert_eq!(failure.detail(37), error.detail(37));
        drop((failed, client, response));
        driver.runtime.release(route);
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        foreign.terminate();
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn calls_before_and_after_foreign_resolution_keep_original_order() {
    use crate::scoped::wire::foreign::{self, ForeignPromises};
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(24, 24);
        let mut foreign = ForeignPromises::new(4);
        let (sender, receiver) = shared::channel();
        let source: capability::Client =
            crate::new_future_client(
                async move { receiver.await.map_err(crate::canceled_to_error) },
            );
        let route = foreign.import(&mut driver.runtime, &*source.hook).unwrap();
        let client = hooks
            .export_client_with_metadata(&mut driver.runtime, &route, foreign::metadata)
            .unwrap();
        let mut before = request(&client, 1).send();
        let mut calls = Calls::new(4, None).unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        assert!(Pin::new(&mut before.promise)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let recorded = Rc::new(RefCell::new(Vec::new()));
        let seen = recorded.clone();
        let destination = capability::Client::new(
            server(move |method, _, _| {
                seen.borrow_mut().push(method);
                ok()
            })
            .add_ref(),
        );
        sender.send(destination).ok().unwrap();
        for _ in 0..16 {
            if let Poll::Ready(result) = foreign.poll(
                &mut hooks,
                &mut driver.runtime,
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(1).unwrap(),
            ) {
                result.unwrap();
            }
            if foreign.is_empty() {
                break;
            }
        }
        assert!(foreign.is_empty());
        let mut after = request(&client, 2).send();
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        ready(&mut before.promise).unwrap();
        ready(&mut after.promise).unwrap();
        assert_eq!(*recorded.borrow(), [1, 2]);
        drop((source, client, before, after));
        driver.runtime.release(route);
        drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
        foreign.terminate();
        teardown(calls, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn unprojected_reply_delivers_directly_and_preserves_late_capabilities() {
    use capntproto_rpc_wire::rpc_capnp::api::{CapDescriptor, Message, Payload, Return};
    for project_early in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(24, 24);
            let client = remote(&mut driver, &mut hooks);
            let capability::RemotePromise {
                mut promise,
                pipeline,
            } = request(&client, 1).send();
            let early = project_early
                .then(|| capability::Client::new(pipeline.hook.get_pipelined_cap(&[])));
            let mut calls = Calls::new(8, None).unwrap();
            let mut queue = messages::Queue::new_growing(0, 8).unwrap();
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            let entry = calls.entries.iter().flatten().next().unwrap();
            assert!(entry.wire.pipeline.is_some());
            let Some(DirectStage::Sent(question)) = entry.wire.direct else {
                panic!("published parent")
            };
            assert_eq!(calls.immediate_receiver(question).is_some(), !project_early);
            drain(&mut queue, messages);
            let reply = Frame::build(messages, 256, Default::default(), |builder| {
                calls::dynamic_results(
                    builder,
                    capntproto_rpc_engine::ids::AnswerId::from_wire(question.id().to_wire()),
                    |content| content.set_capability_index(0),
                )?;
                let mut root = builder.try_get_root::<Message>()?;
                let mut result = root.field(Message::RETURN).edit()?;
                let mut payload = result.field(Return::RESULTS).edit()?;
                payload
                    .field(Payload::CAP_TABLE)
                    .init(1)?
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_HOSTED)
                    .set(73);
                Ok(())
            })
            .unwrap();
            driver
                .receive(reply, messages, &mut [0; 24], |_| (), || ())
                .unwrap_or_else(|rejected| panic!("Return: {:?}", rejected.error));
            let completed = calls
                .returned(
                    question,
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut Context::from_waker(Waker::noop()),
                    |hooks, driver| hooks.collect(&mut driver.runtime),
                )
                .unwrap();
            assert_eq!(completed, (!project_early).then_some(false));
            if project_early {
                for _ in 0..64 {
                    driver
                        .poll_maintenance(
                            messages,
                            &mut queue,
                            128,
                            Default::default(),
                            &mut [0; 24],
                            NonZeroUsize::new(8).unwrap(),
                        )
                        .unwrap();
                    hooks.collect(&mut driver.runtime);
                }
                drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            }
            let response = ready(&mut promise).unwrap();
            let returned: capability::Client = response.get().unwrap().get_as_capability().unwrap();
            let late = capability::Client::new(pipeline.hook.get_pipelined_cap(&[]));
            drop((response, promise, pipeline, early));
            hooks.collect(&mut driver.runtime);
            drain(&mut queue, messages);
            let child = request(&late, 2).send();
            drive(&mut calls, &mut hooks, &mut driver, messages, &mut queue);
            let bytes = drain(&mut queue, messages);
            let message =
                capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
            let rpc_capnp::message::Call(call) = message
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("call through retained result capability")
            };
            let call = call.unwrap();
            assert_eq!(call.get_method_id(), 2);
            let rpc_capnp::message_target::ImportedCap(id) =
                call.get_target().unwrap().which().unwrap()
            else {
                panic!("returned imported capability")
            };
            assert_eq!(id, 73);
            drop((child, returned, late, client));
            teardown(calls, hooks, driver, messages, &mut queue);
        });
    }
}
