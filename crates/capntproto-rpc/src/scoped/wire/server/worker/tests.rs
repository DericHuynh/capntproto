use super::*;
use capntproto_rpc_engine::{
    connection as storage,
    ids::{ImportId, QuestionId, WireId},
    io,
};
use capntproto_rpc_wire::{calls, owned::Frame, rpc_capnp};
use futures::FutureExt;

type Service =
    dyn Fn(u16, capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult;
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
        _: capability::Params<any_pointer::Owned>,
        results: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        (self.0)(method, results)
    }
}
fn server(
    f: impl Fn(u16, capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult + 'static,
) -> Rc<dyn ClientHook> {
    Rc::new(crate::local::Client::new(Server(Rc::new(f))))
}
type TestPeer<'p, 'id, 's> = Peer<'p, 'id, 's, (), u8, 4, 4>;
type TestWorker<'p, 'id> = Worker<'p, 'id, (), 4, 4>;
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
                let mut slots = connection::FixedStorage::<
                    (),
                    Results,
                    u8,
                    Rc<dyn ClientHook>,
                    4,
                    4,
                    16,
                    64,
                >::new();
                f(slots.bind(domain, 1).unwrap(), messages);
            })
        },
    )
    .unwrap();
}
fn expose(driver: &mut TestPeer<'_, '_, '_>, hook: Rc<dyn ClientHook>) -> ImportId {
    let target = driver
        .runtime
        .endpoint(Route::local(hook), clients::Location::Local)
        .unwrap_or_else(|e| panic!("{}", e.error));
    let id = {
        let mut batch = driver.ledgers.exports.batch();
        let id = capntproto_rpc_wire::clients::stage(&mut driver.runtime, &mut batch, &target)
            .unwrap()
            .id();
        batch.commit();
        id
    };
    driver.runtime.release(target);
    ImportId::from_wire(id.to_wire())
}
fn incoming<'p, 'id>(
    driver: &mut TestPeer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    question: u32,
    method: u16,
    target: calls::Target<'_>,
) -> answers::Key<'id> {
    let built = Frame::build(messages, 512, Default::default(), |builder| {
        calls::dynamic_call(
            builder,
            QuestionId::from_wire(question),
            target,
            DynamicCall {
                interface: 7,
                method,
                ..Default::default()
            },
            |_| Ok(()),
        )
    })
    .unwrap();
    let bytes = built
        .with(messages, |root| {
            let mut message = capnp::message::Builder::new_default();
            message.set_root::<rpc_capnp::message::Owned>(root).unwrap();
            capnp::serialize::write_message_to_words(&message)
        })
        .unwrap();
    built.release(messages);
    // The worker receives an envelope through caller-polled fragmented IO.
    struct Source {
        bytes: Vec<u8>,
        position: usize,
        polls: usize,
    }
    impl io::Read for Source {
        type Error = ();
        fn poll_read(
            &mut self,
            cx: &mut Context<'_>,
            output: &mut [u8],
        ) -> Poll<Result<usize, io::IoError<()>>> {
            self.polls += 1;
            if self.polls.is_multiple_of(5) {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            if self.polls.is_multiple_of(7) {
                return Poll::Ready(Err(io::IoError::Interrupted));
            }
            let n = output.len().min(3).min(self.bytes.len() - self.position);
            output[..n].copy_from_slice(&self.bytes[self.position..self.position + n]);
            self.position += n;
            Poll::Ready(Ok(n))
        }
    }
    let mut source = Source {
        bytes,
        position: 0,
        polls: 0,
    };
    let writer = messages.reserve(512).unwrap();
    let frame = {
        let receive = owned::tasks::receive(
            messages.write_owned(writer).unwrap(),
            &mut source,
            Default::default(),
            NonZeroUsize::new(2).unwrap(),
        );
        let mut receive = std::pin::pin!(receive);
        let mut complete = None;
        for _ in 0..1024 {
            if let Poll::Ready(received) = receive
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            {
                complete = Some(received);
                break;
            }
        }
        complete
            .expect("input progresses")
            .finish(messages)
            .unwrap()
            .unwrap()
    };
    assert_eq!(source.position, source.bytes.len());
    match driver
        .receive(frame, messages, &mut [0; 16], |_| (), Results::new)
        .unwrap_or_else(|r| panic!("{}", r.error))
    {
        connection::Received::Queued(key) => key,
        _ => panic!("incoming Call not queued"),
    }
}
fn finish<'p>(
    driver: &mut TestPeer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    id: u32,
) {
    let frame = Frame::build(messages, 128, Default::default(), |b| {
        calls::finish(b, QuestionId::from_wire(id), true)
    })
    .unwrap();
    driver
        .receive(frame, messages, &mut [0; 16], |_| (), Results::new)
        .unwrap_or_else(|r| panic!("{}", r.error));
}
fn import<'p, 'id>(
    runtime: &mut Runtime<'p, 'id, '_, (), u8, Rc<dyn ClientHook>, 4, 4>,
    hook: &dyn ClientHook,
) -> capnp::Result<Client<'id>> {
    runtime
        .endpoint(
            Route::local(Rc::from(hook.add_ref())),
            clients::Location::Local,
        )
        .map_err(|e| wire_error(e.error))
}
fn tick<'p, 'id>(
    worker: &mut TestWorker<'p, 'id>,
    hooks: &mut ClientHooks<'id>,
    driver: &mut TestPeer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> Poll<capnp::Result<Progress>> {
    worker.poll(
        hooks,
        driver,
        messages,
        queue,
        128,
        Default::default(),
        &mut [0; 16],
        &mut Context::from_waker(Waker::noop()),
        NonZeroUsize::new(1).unwrap(),
        import,
    )
}
#[derive(Default)]
struct Sink {
    bytes: Vec<u8>,
    polls: usize,
}

#[cfg(all(unix, not(miri)))]
#[test]
fn imported_metadata_is_ready_during_synchronous_server_dispatch() {
    use rpc_capnp::api::{Call, CapDescriptor, Message, MessageTarget, Payload};
    #[derive(Clone)]
    struct Inspect(Rc<Cell<usize>>);
    impl capability::Server for Inspect {
        fn as_ptr(&self) -> usize {
            Rc::as_ptr(&self.0) as usize
        }
        fn dispatch_call(
            self,
            _: u64,
            _: u16,
            params: capability::Params<any_pointer::Owned>,
            _: capability::Results<any_pointer::Owned>,
        ) -> capability::DispatchCallResult {
            let cap: capability::Client = params.get().unwrap().get_as_capability().unwrap();
            assert!(
                cap.hook.get_fd().is_some(),
                "metadata must precede synchronous dispatch"
            );
            self.0.set(self.0.get() + 1);
            capability::DispatchCallResult::new(Promise::ok(()), false)
        }
    }
    with_peer(|mut driver, messages| {
        let dispatched = Rc::new(Cell::new(0));
        let id = expose(
            &mut driver,
            Rc::new(crate::local::Client::new(Inspect(dispatched.clone()))),
        );
        let frame = Frame::build(messages, 512, Default::default(), |builder| {
            let root = builder.try_init_root::<Message>()?;
            let mut call = root.into_field(Message::CALL).init()?;
            call.scalar(Call::QUESTION_ID).set(0);
            call.field(Call::TARGET)
                .init()?
                .scalar(MessageTarget::IMPORTED_CAP)
                .set(id.to_wire());
            let mut payload = call.into_field(Call::PARAMS).init()?;
            payload.field(Payload::CONTENT).set_capability_index(0)?;
            payload
                .field(Payload::CAP_TABLE)
                .init(1)?
                .get_mut(0)
                .unwrap()
                .scalar(CapDescriptor::SENDER_HOSTED)
                .set(7);
            Ok(())
        })
        .unwrap();
        driver
            .receive(frame, messages, &mut [0; 16], |_| (), Results::new)
            .unwrap_or_else(|_| panic!("admit"));
        let mut worker = Worker::new(1, None);
        let mut hooks = ClientHooks::new(16, 16);
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        let (fd, _other) = std::os::unix::net::UnixStream::pair().unwrap();
        let fd = Rc::new(std::os::fd::OwnedFd::from(fd));
        assert!(worker.accept(&mut driver).unwrap());
        for _ in 0..4 {
            assert!(matches!(
                worker.poll_using(
                    &mut hooks,
                    &mut driver,
                    messages,
                    512,
                    Default::default(),
                    &mut Context::from_waker(Waker::noop()),
                    NonZeroUsize::new(1).unwrap(),
                    import,
                    |hooks, driver| hooks.collect_wire(
                        driver,
                        |_| EndpointMetadata {
                            failure: None,
                            introduction: None,
                            fd: Some(fd.clone())
                        },
                        foreign::local_metadata
                    ),
                    |_, _, _, _| panic!("dispatch precedes publication"),
                    |_| {},
                    |_, _, _, _| Ok(false),
                ),
                Poll::Ready(Ok(Progress::Advanced))
            ));
            if dispatched.get() != 0 {
                break;
            }
        }
        assert_eq!(dispatched.get(), 1);
        close(worker, hooks, driver, messages, &mut queue);
    });
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
        if self.polls.is_multiple_of(7) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if self.polls.is_multiple_of(11) {
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
        match io::poll_queue(
            &mut sink,
            queue,
            messages,
            &mut Context::from_waker(Waker::noop()),
            NonZeroUsize::new(3).unwrap(),
        ) {
            Poll::Ready(result) => {
                result.unwrap();
                return sink.bytes;
            }
            Poll::Pending => (),
        }
    }
    panic!("output failed to drain");
}
fn check(bytes: &[u8], f: impl FnOnce(rpc_capnp::return_::Reader<'_>)) {
    let mut bytes = bytes;
    let message = capnp::serialize::read_message(&mut bytes, Default::default()).unwrap();
    assert!(bytes.is_empty());
    let rpc_capnp::message::Return(returned) = message
        .get_root::<rpc_capnp::message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    f(returned.unwrap());
}
fn close<'p, 'id>(
    worker: TestWorker<'p, 'id>,
    hooks: ClientHooks<'id>,
    mut driver: TestPeer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    worker.terminate(&mut driver, messages);
    hooks.close(&mut driver.runtime, Error::disconnected("closed".into()));
    let mut closing = driver.terminate(queue);
    for _ in 0..1024 {
        if matches!(
            closing
                .poll(messages, NonZeroUsize::new(16).unwrap())
                .unwrap(),
            connection::Termination::Done
        ) {
            // No request, published result or output send keeps a buffer leased.
            let mut buffers = Vec::new();
            for _ in 0..32 {
                buffers.push(messages.reserve(16).unwrap());
            }
            for buffer in buffers {
                messages.discard(buffer);
            }
            return;
        }
    }
    panic!("terminal cleanup");
}

#[test]
fn worker_retries_large_replies_without_reexecuting_and_preserves_admission() {
    const SIZE: usize = if cfg!(miri) { 257 } else { 65536 };
    with_peer(|mut driver, messages| {
        let calls = Rc::new(Cell::new(0));
        let counted = calls.clone();
        let id = expose(
            &mut driver,
            server(move |method, mut results| {
                counted.set(counted.get() + 1);
                results
                    .get()
                    .set_as::<capnp::data::Owned>(vec![method as u8; SIZE].as_slice())
                    .unwrap();
                capability::DispatchCallResult::new(Promise::ok(()), false)
            }),
        );
        let mut worker = Worker::new(1, None);
        let mut hooks = ClientHooks::new(64, 16);
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        incoming(&mut driver, messages, 0, 3, calls::Target::Imported(id));
        incoming(&mut driver, messages, 1, 7, calls::Target::Imported(id));
        assert!(worker.accept(&mut driver).unwrap());
        let next = driver.runtime.next_ready().unwrap().unwrap().key();
        assert!(!worker.accept(&mut driver).unwrap());
        assert_eq!(driver.runtime.next_ready().unwrap().unwrap().key(), next);
        // Fill the FIFO independently; the application must still complete.
        let writer = messages.reserve(16).unwrap();
        messages.write(&writer)[..16].fill(0);
        messages.write(&writer)[4] = 1;
        let frame = messages.seal(writer, 16).unwrap_or_else(|_| panic!());
        let offer = messages.offer(&frame).unwrap();
        queue.reserve().unwrap().accept(messages, offer);
        messages.release(frame);
        let mut blocked = false;
        for _ in 0..16 {
            if matches!(
                tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
                Poll::Ready(Err(_))
            ) {
                blocked = true;
                break;
            }
        }
        assert!(blocked);
        assert_eq!(calls.get(), 1);
        drain(&mut queue, messages);
        assert!(matches!(
            tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
            Poll::Ready(Ok(Progress::Replied))
        ));
        check(&drain(&mut queue, messages), |returned| {
            assert_eq!(returned.get_answer_id(), 0);
            let rpc_capnp::return_::Results(results) = returned.which().unwrap() else {
                panic!()
            };
            let data = results
                .unwrap()
                .get_content()
                .get_as::<capnp::data::Reader>()
                .unwrap();
            assert_eq!(data, vec![3; SIZE]);
        });
        assert!(worker.accept(&mut driver).unwrap());
        for _ in 0..16 {
            if matches!(
                tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
                Poll::Ready(Ok(Progress::Replied))
            ) {
                break;
            }
        }
        assert!(worker.is_empty());
        assert_eq!(calls.get(), 2);
        check(&drain(&mut queue, messages), |r| {
            assert_eq!(r.get_answer_id(), 1)
        });
        finish(&mut driver, messages, 0);
        finish(&mut driver, messages, 1);
        close(worker, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn pending_service_does_not_starve_ready_reply_or_shutdown() {
    with_peer(|mut driver, messages| {
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let id = expose(
            &mut driver,
            server(move |method, mut results| {
                seen.set(seen.get() + 1);
                if method == 0 {
                    return capability::DispatchCallResult::with_cancellation_policy(
                        Promise::from_future(futures::future::pending()),
                        false,
                        true,
                    );
                }
                results.get().set_as::<capnp::text::Owned>("ready").unwrap();
                capability::DispatchCallResult::new(Promise::ok(()), false)
            }),
        );
        let mut worker = Worker::new(2, None);
        let mut hooks = ClientHooks::new(64, 16);
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        incoming(&mut driver, messages, 0, 0, calls::Target::Imported(id));
        incoming(&mut driver, messages, 1, 1, calls::Target::Imported(id));
        assert!(worker.accept(&mut driver).unwrap());
        assert!(worker.accept(&mut driver).unwrap());
        let mut replied = false;
        for _ in 0..32 {
            if matches!(
                tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
                Poll::Ready(Ok(Progress::Replied))
            ) {
                replied = true;
                break;
            }
        }
        assert!(replied);
        assert_eq!(count.get(), 2);
        assert!(!worker.is_empty());
        check(&drain(&mut queue, messages), |r| {
            assert_eq!(r.get_answer_id(), 1)
        });
        close(worker, hooks, driver, messages, &mut queue);
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
fn finish_cancels_interest_but_preserves_protected_execution() {
    for (early, cancellable) in [(true, true), (false, true), (false, false)] {
        with_peer(|mut driver, messages| {
            let entered = Rc::new(Cell::new(false));
            let completed = Rc::new(Cell::new(false));
            let (sender, receiver) = shared::channel();
            let receiver = Rc::new(RefCell::new(Some(receiver)));
            let entered2 = entered.clone();
            let completed2 = completed.clone();
            let id = expose(
                &mut driver,
                server(move |_, mut results| {
                    entered2.set(true);
                    results
                        .get()
                        .set_as::<capnp::text::Owned>("partial")
                        .unwrap();
                    let receiver = receiver.borrow_mut().take().unwrap();
                    let completed = completed2.clone();
                    capability::DispatchCallResult::with_cancellation_policy(
                        Promise::from_future(async move {
                            receiver.await.map_err(crate::canceled_to_error)?;
                            completed.set(true);
                            drop(results);
                            Ok(())
                        }),
                        false,
                        cancellable,
                    )
                }),
            );
            let executor = Rc::new(Executor::default());
            let mut worker = Worker::new(2, Some(executor.clone()));
            let mut hooks = ClientHooks::new(64, 16);
            let mut queue = messages::Queue::new_growing(0, 2).unwrap();
            incoming(&mut driver, messages, 0, 0, calls::Target::Imported(id));
            if early {
                finish(&mut driver, messages, 0);
            }
            assert!(worker.accept(&mut driver).unwrap());
            if !early {
                for _ in 0..4 {
                    let _ = tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue);
                }
                assert!(entered.get());
                finish(&mut driver, messages, 0);
            }
            for _ in 0..16 {
                if matches!(
                    tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
                    Poll::Ready(Ok(Progress::Replied))
                ) {
                    break;
                }
            }
            assert!(worker.is_empty());
            assert_eq!(entered.get(), !early);
            check(&drain(&mut queue, messages), |r| {
                assert!(matches!(
                    r.which().unwrap(),
                    rpc_capnp::return_::Canceled(())
                ))
            });
            if !early && !cancellable {
                assert!(sender.send(()).is_ok());
                let task = executor.0.borrow_mut().pop().expect("protected execution");
                assert!(task.now_or_never().unwrap().is_ok());
                assert!(completed.get());
            } else {
                assert!(executor.0.borrow().is_empty());
                assert!(!completed.get());
            }
            close(worker, hooks, driver, messages, &mut queue);
        });
    }
}

#[test]
fn exception_sizing_preserves_large_reason_and_detail() {
    const REASON: usize = if cfg!(miri) { 129 } else { 8193 };
    const DETAIL: usize = if cfg!(miri) { 257 } else { 16385 };
    with_peer(|mut driver, messages| {
        let id = expose(
            &mut driver,
            server(|_, _| {
                let mut error = Error::overloaded("x".repeat(REASON));
                error.set_detail(79, vec![0x63; DETAIL]);
                capability::DispatchCallResult::new(Promise::err(error), false)
            }),
        );
        let mut worker = Worker::new(1, None);
        let mut hooks = ClientHooks::new(64, 16);
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        incoming(&mut driver, messages, 0, 0, calls::Target::Imported(id));
        assert!(worker.accept(&mut driver).unwrap());
        for _ in 0..16 {
            match tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue) {
                Poll::Ready(Ok(Progress::Replied)) => break,
                Poll::Ready(Err(e)) => panic!("{e}"),
                _ => (),
            }
        }
        assert!(worker.is_empty());
        check(&drain(&mut queue, messages), |r| {
            let rpc_capnp::return_::Exception(error) = r.which().unwrap() else {
                panic!()
            };
            let error = crate::rpc::remote_exception_to_error(error.unwrap());
            assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
            assert_eq!(
                error.extra,
                format!("remote exception: {}", "x".repeat(REASON))
            );
            assert_eq!(
                error.details().next().unwrap(),
                (79, vec![0x63; DETAIL].as_slice())
            );
        });
        close(worker, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn worker_dispatches_early_pipeline_before_parent_completion_and_keeps_failure_details() {
    with_peer(|mut driver, messages| {
        let child = server(|_, mut r| {
            r.get().set_as::<capnp::text::Owned>("child").unwrap();
            capability::DispatchCallResult::new(Promise::ok(()), false)
        });
        let (sender, receiver) = shared::channel::<()>();
        let receiver = Rc::new(RefCell::new(Some(receiver)));
        let parent = expose(
            &mut driver,
            server(move |_, mut r| {
                r.get().set_as_capability(child.add_ref());
                r.set_pipeline().unwrap();
                let receiver = receiver.borrow_mut().take().unwrap();
                capability::DispatchCallResult::new(
                    Promise::from_future(async move {
                        receiver.await.map_err(crate::canceled_to_error)?;
                        drop(r);
                        Err(Error::overloaded("parent failed after pipeline".into()))
                    }),
                    false,
                )
            }),
        );
        let mut worker = Worker::new(2, None);
        let mut hooks = ClientHooks::new(64, 16);
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        incoming(&mut driver, messages, 0, 0, calls::Target::Imported(parent));
        incoming(
            &mut driver,
            messages,
            1,
            0,
            calls::Target::Promised {
                question: QuestionId::from_wire(0),
                path: pipeline::Path::new(&[]),
            },
        );
        assert!(worker.accept(&mut driver).unwrap());
        assert!(!worker.accept(&mut driver).unwrap());
        let mut child_replied = false;
        for _ in 0..512 {
            let progress = driver
                .poll_maintenance_with(
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 16],
                    NonZeroUsize::new(16).unwrap(),
                    &mut || (),
                    &mut |_, _, _| u8::MAX,
                    &mut |results, path, runtime| {
                        hooks
                            .server_pipeline(results, path, runtime, import)
                            .map(|client| client.map(Ok))
                            .map_err(|error| {
                                owned::pipelines::Error::Input(owned::Error::Wire(error.kind))
                            })
                    },
                )
                .unwrap();
            drop(progress);
            worker.accept(&mut driver).unwrap();
            match tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue) {
                Poll::Ready(Err(error)) => panic!("{error}"),
                Poll::Ready(Ok(Progress::Replied)) => {
                    child_replied = true;
                    break;
                }
                _ => (),
            }
        }
        assert!(
            child_replied,
            "child must finish while parent is still pending"
        );
        check(&drain(&mut queue, messages), |r| {
            assert_eq!(r.get_answer_id(), 1);
            let rpc_capnp::return_::Results(results) = r.which().unwrap() else {
                panic!()
            };
            assert_eq!(
                results
                    .unwrap()
                    .get_content()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "child"
            );
        });
        assert!(sender.send(()).is_ok());
        for _ in 0..16 {
            if matches!(
                tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
                Poll::Ready(Ok(Progress::Replied))
            ) {
                break;
            }
        }
        assert!(worker.is_empty());
        check(&drain(&mut queue, messages), |r| {
            assert_eq!(r.get_answer_id(), 0);
            let rpc_capnp::return_::Exception(exception) = r.which().unwrap() else {
                panic!()
            };
            assert_eq!(
                crate::rpc::remote_exception_to_error(exception.unwrap()).extra,
                "remote exception: parent failed after pipeline"
            );
        });
        close(worker, hooks, driver, messages, &mut queue);
    });
}

#[test]
fn worker_resources_follow_partial_writes_once_and_survive_capacity_failure() {
    use capntproto_rpc_wire::attachments::Sender;
    struct Resources {
        sink: Sink,
        attachments: Vec<u32>,
    }
    impl capntproto_rpc_engine::io::Flush for Resources {
        type Error = ();
        fn poll_flush(
            &mut self,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), capntproto_rpc_engine::io::IoError<Self::Error>>> {
            core::task::Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<Sender<u32, 4>> for Resources {
        fn poll_write_resources(
            &mut self,
            cx: &mut Context<'_>,
            bytes: &[u8],
            resources: Option<&Sender<u32, 4>>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            use io::Write;
            let result = self.sink.poll_write(cx, bytes);
            if matches!(result, Poll::Ready(Ok(n)) if n > 0) {
                if let Some(resources) = resources {
                    self.attachments.extend(resources.iter().copied());
                }
            }
            result
        }
    }
    with_peer(|mut driver, messages| {
        let child = server(|_, _| capability::DispatchCallResult::new(Promise::ok(()), false));
        let id = expose(
            &mut driver,
            server(move |_, mut r| {
                r.get().set_as_capability(child.add_ref());
                capability::DispatchCallResult::new(Promise::ok(()), false)
            }),
        );
        let mut worker = Worker::new(1, None);
        let mut hooks = ClientHooks::new(64, 16);
        let mut queue: messages::Queue<'_, '_, Sender<u32, 4>> =
            messages::Queue::growing_with_resources(0, 2).unwrap();
        incoming(&mut driver, messages, 0, 0, calls::Target::Imported(id));
        assert!(worker.accept(&mut driver).unwrap());
        let mut rejected = false;
        for _ in 0..16 {
            if matches!(
                worker.poll_with_attachments(
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
                    |_| Some(73),
                    import
                ),
                Poll::Ready(Err(_))
            ) {
                rejected = true;
                break;
            }
        }
        assert!(rejected, "export scratch failure must retain the result");
        assert!(queue.is_empty());
        assert!(matches!(
            worker.poll_with_attachments(
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                128,
                Default::default(),
                &mut [0; 16],
                &mut Context::from_waker(Waker::noop()),
                NonZeroUsize::new(1).unwrap(),
                capntproto_rpc_wire::Capacity::Inline,
                |_| None,
                |_| Some(73),
                import
            ),
            Poll::Ready(Ok(Progress::Replied))
        ));
        let mut output = Resources {
            sink: Sink {
                polls: 6,
                ..Default::default()
            },
            attachments: Vec::new(),
        };
        for _ in 0..512 {
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
        assert_eq!(output.attachments, [73]);
        check(&output.sink.bytes, |r| {
            let rpc_capnp::return_::Results(results) = r.which().unwrap() else {
                panic!()
            };
            assert_eq!(
                results
                    .unwrap()
                    .get_cap_table()
                    .unwrap()
                    .get(0)
                    .get_attached_fd(),
                0
            );
        });
        worker.terminate(&mut driver, messages);
        hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
        let mut closing = driver.terminate(&mut queue);
        for _ in 0..1024 {
            if matches!(
                closing
                    .poll(messages, NonZeroUsize::new(16).unwrap())
                    .unwrap(),
                connection::Termination::Done
            ) {
                return;
            }
        }
        panic!("resource cleanup");
    });
}

#[test]
fn bounded_poll_registers_every_pending_method_and_stops_after_disconnect() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Wake(AtomicUsize);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    with_peer(|mut driver, messages| {
        let waiters = Rc::new(RefCell::new(vec![None::<Waker>, None]));
        let saved = waiters.clone();
        let id = expose(
            &mut driver,
            server(move |method, results| {
                let saved = saved.clone();
                capability::DispatchCallResult::with_cancellation_policy(
                    Promise::from_future(futures::future::poll_fn(move |cx| {
                        let _ = &results;
                        saved.borrow_mut()[method as usize] = Some(cx.waker().clone());
                        Poll::Pending
                    })),
                    false,
                    true,
                )
            }),
        );
        let mut worker = Worker::new(2, None);
        let mut hooks = ClientHooks::new(64, 16);
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        incoming(&mut driver, messages, 0, 0, calls::Target::Imported(id));
        incoming(&mut driver, messages, 1, 1, calls::Target::Imported(id));
        assert!(worker.accept(&mut driver).unwrap());
        assert!(worker.accept(&mut driver).unwrap());
        let wake = Arc::new(Wake(AtomicUsize::new(0)));
        let waker = Waker::from(wake.clone());
        let mut cx = Context::from_waker(&waker);
        for _ in 0..2 {
            assert!(matches!(
                worker.poll(
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    &mut [0; 16],
                    &mut cx,
                    NonZeroUsize::new(1).unwrap(),
                    import
                ),
                Poll::Ready(Ok(Progress::Advanced))
            ));
        }
        assert!(matches!(
            worker.poll(
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                128,
                Default::default(),
                &mut [0; 16],
                &mut cx,
                NonZeroUsize::new(1).unwrap(),
                import
            ),
            Poll::Ready(Ok(Progress::Yield))
        ));
        assert_eq!(
            wake.0.load(Ordering::SeqCst),
            0,
            "the coordinator schedules a reported Yield"
        );
        let before = wake.0.load(Ordering::SeqCst);
        assert!(worker
            .poll(
                &mut hooks,
                &mut driver,
                messages,
                &mut queue,
                128,
                Default::default(),
                &mut [0; 16],
                &mut cx,
                NonZeroUsize::new(1).unwrap(),
                import
            )
            .is_pending());
        assert_eq!(
            wake.0.load(Ordering::SeqCst),
            before,
            "quiescent worker must not spin"
        );
        assert!(waiters.borrow().iter().all(Option::is_some));
        waiters.borrow()[1].as_ref().unwrap().wake_by_ref();
        assert_eq!(wake.0.load(Ordering::SeqCst), before + 1);
        driver.disconnect(capnp::ErrorKind::Disconnected);
        assert!(worker.accept(&mut driver).is_err());
        assert!(matches!(
            tick(&mut worker, &mut hooks, &mut driver, messages, &mut queue),
            Poll::Ready(Err(_))
        ));
        close(worker, hooks, driver, messages, &mut queue);
    });
}
