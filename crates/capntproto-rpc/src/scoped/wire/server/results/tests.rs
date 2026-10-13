use super::*;
use capntproto_rpc_engine::{
    clients, connection as storage,
    ids::{ImportId, QuestionId, WireId},
};
use capntproto_rpc_wire::{
    clients::Route,
    owned::{pipelines, Frame},
    pipeline, rpc_capnp,
};

#[derive(Clone)]
struct Server(
    Rc<dyn Fn(capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult>,
);
impl capability::Server for Server {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as *const () as usize
    }
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        _: capability::Params<any_pointer::Owned>,
        results: capability::Results<any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        (self.0)(results)
    }
}
fn server(
    f: impl Fn(capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult + 'static,
) -> capability::Client {
    capability::Client::new(Box::new(crate::local::Client::new(Server(Rc::new(f)))))
}
fn prepared() -> Prepared {
    Prepared {
        method: DynamicCall {
            interface: 7,
            method: 9,
            ..Default::default()
        },
        params: Payload::owned(Default::default(), vec![]),
    }
}

#[test]
fn returned_results_skip_ready_waiters_but_pending_work_and_early_pipelines_still_wake() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let wake = Arc::new(Count::default());
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    for pending in [false, true] {
        let results = Results::new();
        let service = server(move |r| {
            drop(r);
            capability::DispatchCallResult::with_cancellation_policy(
                if pending {
                    Promise::from_future(core::future::pending())
                } else {
                    Promise::ok(())
                },
                false,
                true,
            )
        });
        let mut execution = prepared().execute(&*service.hook, &results, None).unwrap();
        assert!(results.0.payload.borrow().is_none());
        wake.0.store(0, Ordering::SeqCst);
        assert_eq!(
            results.poll_work(&mut execution, &mut cx).is_pending(),
            pending
        );
        assert!(results.0.payload.borrow().is_some());
        assert_eq!(wake.0.load(Ordering::SeqCst), 0);
        if pending {
            results.fail(Error::failed("external completion".into()));
            assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        } else {
            assert!(results.0.wake.borrow().is_empty());
            assert_eq!(results.0.wake.borrow().capacity(), 0);
        }
    }

    // The application still owns the builder and publishes during a poll which
    // returns Pending. An observer must wake even without an execution wake.
    let results = Results::new();
    let mut hook = Hook {
        results: results.clone(),
        message: None,
        caps: vec![],
        executor: None,
    };
    wake.0.store(0, Ordering::SeqCst);
    let mut published = false;
    let mut work = core::future::poll_fn(|_| {
        if !published {
            hook.set_pipeline().unwrap();
            published = true;
        }
        Poll::<()>::Pending
    });
    assert!(results.poll_work(&mut work, &mut cx).is_pending());
    assert_eq!(wake.0.load(Ordering::SeqCst), 1);
    assert!(results.early_pipeline().is_some());
    wake.0.store(0, Ordering::SeqCst);
    assert!(results.poll_work(&mut work, &mut cx).is_pending());
    assert_eq!(wake.0.load(Ordering::SeqCst), 0);

    // A completed service can retain its builder. Collection must still wait
    // for the hook, and hook return must wake the collector.
    let results = Results::new();
    let hook = Hook {
        results: results.clone(),
        message: None,
        caps: vec![],
        executor: None,
    };
    let mut execution = Execution {
        operation: Some(Promise::ok(())),
        results: results.clone(),
    };
    assert!(matches!(
        results.poll_work(&mut execution, &mut cx),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(wake.0.load(Ordering::SeqCst), 0);
    assert!(results.poll_completed(&mut cx).is_pending());
    drop(hook);
    assert_eq!(wake.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        results.poll_completed(&mut cx),
        Poll::Ready(Ok(()))
    ));
}

#[test]
fn transferred_pipeline_waits_for_results_retained_after_execution_completes() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl std::task::Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let wake = Arc::new(Count::default());
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    let results = Results::new();
    let mut hook = Hook {
        results: results.clone(),
        message: None,
        caps: vec![],
        executor: None,
    };
    hook.get()
        .unwrap()
        .set_as_capability(crate::broken::new_cap(Error::failed("returned cap".into())));
    let pipeline = TransferPipeline {
        results: results.clone(),
        work: Execution {
            operation: Some(Promise::ok(())),
            results: results.clone(),
        }
        .boxed_local()
        .shared(),
    };
    let client = pipeline.get_pipelined_cap(&[]);
    let mut resolution = client.when_resolved();
    // Shared completion can already have notified its parent. Consume that
    // notification before checking for a distinct wake from returning the hook.
    for _ in 0..8 {
        wake.0.store(0, Ordering::SeqCst);
        assert!(Pin::new(&mut resolution).poll(&mut cx).is_pending());
        if wake.0.load(Ordering::SeqCst) == 0 {
            break;
        }
    }
    assert_eq!(wake.0.load(Ordering::SeqCst), 0);
    assert!(results.0.completion.borrow().is_some());
    assert!(results.0.payload.borrow().is_none());
    wake.0.store(0, Ordering::SeqCst);
    drop(hook);
    assert!(wake.0.load(Ordering::SeqCst) > 0);
    assert!(matches!(
        Pin::new(&mut resolution).poll(&mut cx),
        Poll::Ready(Err(error)) if error.extra == "returned cap"
    ));
}

#[test]
fn pooled_results_keep_retained_payloads_and_clear_reused_segments() {
    for cache_words in [0, 512] {
        let pool = capnp::message::SegmentPool::new(cache_words, 2);
        let build = |value| {
            let results = Results::with_charge(None, Some(pool.clone()));
            let mut hook = Hook {
                results: results.clone(),
                message: None,
                caps: Vec::new(),
                executor: None,
            };
            let data = hook.get().unwrap().initn_as::<capnp::data::Builder>(1200);
            assert!(data.iter().all(|byte| *byte == 0));
            data.fill(value);
            drop(hook);
            let payload = results.0.payload.borrow().as_ref().unwrap().clone();
            payload
        };
        let retained = build(0xa5);
        for value in 0..16 {
            let next = build(value);
            assert!(next
                .reader()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap()
                .iter()
                .all(|byte| *byte == value));
            assert!(retained
                .reader()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap()
                .iter()
                .all(|byte| *byte == 0xa5));
        }
    }
}

#[test]
fn prepared_return_rejects_capability_content_before_reserving_storage() {
    let mut source = connection::PreparedReturn::from_template(
        &connection::PreparedReturnTemplate::default(),
        capnp::message::HeapAllocator::new(),
    );
    let mut caps = Vec::new();
    let mut root = source.results();
    root.imbue_mut(&mut caps);
    root.set_as_capability(crate::broken::new_cap(Error::failed(
        "test capability".into(),
    )));
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, Results::new());
        assert!(driver
            .local_answer_prepared(call.answer(), 7, 9, &source, messages, Default::default())
            .is_err());
        assert!(driver.ledgers.answers.prepare_return(call.answer()).is_ok());
        driver.release_call(call, messages);
        driver.runtime.release(target);
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        close(driver, messages, &mut queue);
    });
}

#[test]
fn exception_trace_is_sized_once_and_reentrant_policy_changes_apply_to_next_result() {
    use rpc_capnp::api::{Message, Return};
    let policy = coordinator::policy::Policy::new();
    let weak = Rc::downgrade(&policy);
    let calls = Rc::new(Cell::new(0));
    let counted = calls.clone();
    let trace = "local trace ".repeat(64);
    let expected = trace.clone();
    policy.set_trace_encoder(move |_| {
        counted.set(counted.get() + 1);
        weak.upgrade().unwrap().clear_trace_encoder();
        trace.clone()
    });
    let results = Results::new();
    let mut error = Error::failed("original reason".into());
    error.set_remote_trace("do not forward this".into());
    results.fail(error);
    results.prepare_trace(&policy);
    results.prepare_trace(&policy); // A publication retry cannot re-run user code.
    assert_eq!(calls.get(), 1);
    with_driver(|_driver, messages| {
        let frame = Frame::build(
            messages,
            results.publication_bytes().unwrap(),
            Default::default(),
            |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut ret = root.field(Message::RETURN).init()?;
                results.write_exception(ret.field(Return::EXCEPTION).init()?)
            },
        )
        .unwrap();
        frame
            .with(messages, |root| {
                let rpc_capnp::message::Return(ret) = root.which().unwrap() else {
                    panic!()
                };
                let rpc_capnp::return_::Exception(exception) = ret.unwrap().which().unwrap() else {
                    panic!()
                };
                let exception = exception.unwrap();
                assert_eq!(exception.get_reason().unwrap(), "original reason");
                assert_eq!(exception.get_trace().unwrap(), expected);
            })
            .unwrap();
        frame.release(messages);
    });
    let next = Results::new();
    next.fail(Error::failed("next".into()));
    next.prepare_trace(&policy);
    assert!(next.0.trace.borrow().is_none());
}
fn poll<F: Future + Unpin>(f: &mut F) -> Poll<F::Output> {
    Pin::new(f).poll(&mut Context::from_waker(Waker::noop()))
}
fn with_driver(
    f: impl for<'p, 'id, 's, 'm, 'b> FnOnce(
        Driver<'p, 'id, 's, (), Results, u8, u32, 4, 4>,
        &mut messages::Messages<'p, 'm, 'b>,
    ),
) {
    let mut buffers = messages::GrowingBuffers::new(12, 4096, 12 * 4103);
    messages::with_growing_messages(
        &mut buffers,
        storage::Growth {
            initial: 0,
            maximum: 8,
        },
        |messages| {
            storage::with_domain(|domain| {
                let mut slots =
                    connection::FixedStorage::<(), Results, u8, u32, 4, 4, 8, 32>::new();
                f(slots.bind(domain, 1).unwrap(), messages);
            });
        },
    )
    .unwrap();
}
fn request<'p, 'id>(
    driver: &mut Driver<'p, 'id, '_, (), Results, u8, u32, 4, 4>,
    messages: &mut messages::Messages<'p, '_, '_>,
    results: Results,
) -> (connection::Request<'p, 'id, (), 4, 4>, Client<'id>) {
    let target = driver
        .runtime
        .endpoint(Route::local(31), clients::Location::Local)
        .unwrap();
    let id = {
        let mut batch = driver.ledgers.exports.batch();
        let id = capntproto_rpc_wire::clients::stage(&mut driver.runtime, &mut batch, &target)
            .unwrap()
            .id();
        batch.commit();
        id
    };
    let frame = Frame::build(messages, 512, Default::default(), |b| {
        capntproto_rpc_wire::calls::dynamic_call(
            b,
            QuestionId::from_wire(3),
            capntproto_rpc_wire::calls::Target::Imported(ImportId::from_wire(id.to_wire())),
            prepared().method,
            |_| Ok(()),
        )
    })
    .unwrap();
    let call = connection::Request::admit(
        frame,
        messages,
        &mut driver.ledgers.imports,
        &mut driver.ledgers.exports,
        &mut driver.ledgers.answers,
        connection::Answer::new(pipelines::Results::new(results)),
        |_| (),
    )
    .unwrap_or_else(|_| panic!("admit"));
    assert!(driver
        .ledgers
        .answers
        .dispatch(call.answer(), true)
        .unwrap());
    (call, target)
}
fn close<'p>(
    driver: Driver<'p, '_, '_, (), Results, u8, u32, 4, 4>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    let mut closing = driver.terminate(queue);
    for _ in 0..256 {
        if matches!(
            closing
                .poll(messages, std::num::NonZeroUsize::new(32).unwrap())
                .unwrap(),
            connection::Termination::Done
        ) {
            return;
        }
    }
    panic!("cleanup");
}

#[test]
fn hosted_reply_preserves_early_pipeline_and_retries_portable_publication() {
    with_driver(|mut driver, messages| {
        let results = Results::new();
        let (call, target) = request(&mut driver, messages, results.clone());
        let value = driver
            .runtime
            .endpoint(Route::local(73), clients::Location::Local)
            .unwrap();
        let mut hooks = ClientHooks::new(16, 16);
        let cap = hooks.export_client(&mut driver.runtime, &value).unwrap();
        let (sender, receiver) = shared::channel::<()>();
        let receiver = Rc::new(RefCell::new(Some(receiver)));
        let service = server(move |mut r| {
            r.get().set_as_capability(cap.hook.add_ref());
            r.set_pipeline().unwrap();
            let ready = receiver.borrow_mut().take().unwrap();
            capability::DispatchCallResult::new(
                Promise::from_future(async move {
                    ready.await.map_err(crate::canceled_to_error)?;
                    drop(r);
                    Ok(())
                }),
                false,
            )
        });
        let mut execution = hooks
            .prepare_received_server(&call, &mut driver, messages)
            .unwrap()
            .execute(&*service.hook, &results, None)
            .unwrap();
        assert!(poll(&mut execution).is_pending());
        assert!(results
            .poll_completed(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observed = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        assert_eq!(
            driver
                .poll_pipeline_with(0, messages, |producer, path, runtime| {
                    let hook = producer.pipeline(path).unwrap();
                    Ok(Some(Ok(hooks
                        .try_retain(runtime, &*hook)
                        .unwrap()
                        .unwrap())))
                })
                .unwrap(),
            connection::Propagation::Resolved
        );
        assert!(sender.send(()).is_ok());
        assert!(matches!(poll(&mut execution), Poll::Ready(Ok(()))));
        assert!(matches!(
            results.poll_completed(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(()))
        ));
        // Exhaust the caller's pool: retry conversion after releasing a buffer.
        let mut reserved = vec![];
        while let Ok(writer) = messages.reserve(4096) {
            reserved.push(writer);
        }
        assert!(hooks
            .collect_server_results(
                &results,
                &mut driver,
                messages,
                Default::default(),
                |_, _| panic!("foreign")
            )
            .is_err());
        for writer in reserved {
            messages.discard(writer);
        }
        let output = hooks
            .collect_server_results(
                &results,
                &mut driver,
                messages,
                Default::default(),
                |_, _| panic!("foreign"),
            )
            .unwrap();
        driver
            .complete_answer(&call, output, messages)
            .unwrap_or_else(|_| panic!("complete"));
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        driver
            .send_answer(
                call.answer(),
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [0; 8],
                Results::write_exception,
            )
            .unwrap();
        let window = queue.window(messages).unwrap().unwrap();
        let mut bytes = window.bytes();
        let message =
            capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, Default::default())
                .unwrap();
        let rpc_capnp::message::Return(returned) = message
            .get_root::<rpc_capnp::message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let rpc_capnp::return_::Results(payload) = returned.unwrap().which().unwrap() else {
            panic!()
        };
        let payload = payload.unwrap();
        assert_eq!(payload.get_content().capability_index().unwrap(), 0);
        assert_eq!(payload.get_cap_table().unwrap().len(), 1);
        let n = window.bytes().len();
        window.advance(n).unwrap();
        driver.runtime.release(observed);
        driver.runtime.release(value);
        driver.runtime.release(target);
        driver.release_call(call, messages);
        drop((results, execution, service));
        hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
        close(driver, messages, &mut queue);
    });
}

#[test]
fn exception_metadata_and_early_pipeline_survive_method_failure() {
    with_driver(|mut driver, messages| {
        let results = Results::new();
        let (call, target) = request(&mut driver, messages, results.clone());
        let mut error = Error::overloaded("specific failure".into());
        error.set_detail(0x1234, vec![7, 8, 9]);
        let service = server(move |mut r| {
            r.get()
                .set_as_capability(crate::broken::new_cap(Error::failed(
                    "early pipeline".into(),
                )));
            r.set_pipeline().unwrap();
            capability::DispatchCallResult::new(Promise::err(error.clone()), false)
        });
        let mut execution = prepared().execute(&*service.hook, &results, None).unwrap();
        assert!(matches!(poll(&mut execution), Poll::Ready(Err(_))));
        // A second start cannot overwrite already completed application state.
        assert!(prepared().execute(&*service.hook, &results, None).is_err());
        let hook = results.pipeline(pipeline::Path::new(&[])).unwrap();
        let failure = hook
            .new_call(1, 2, None)
            .send()
            .promise
            .now_or_never()
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(failure.extra, "early pipeline");
        let mut hooks = ClientHooks::new(8, 8);
        let output = hooks
            .collect_server_results(
                &results,
                &mut driver,
                messages,
                Default::default(),
                |_, _| panic!(),
            )
            .unwrap();
        driver
            .complete_answer(&call, output, messages)
            .unwrap_or_else(|_| panic!());
        let mut queue = messages::Queue::new_growing(0, 2).unwrap();
        driver
            .send_answer(
                call.answer(),
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [],
                Results::write_exception,
            )
            .unwrap();
        let window = queue.window(messages).unwrap().unwrap();
        let mut bytes = window.bytes();
        let message =
            capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, Default::default())
                .unwrap();
        let rpc_capnp::message::Return(returned) = message
            .get_root::<rpc_capnp::message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let rpc_capnp::return_::Exception(exception) = returned.unwrap().which().unwrap() else {
            panic!()
        };
        let exception = exception.unwrap();
        assert_eq!(
            exception.get_reason().unwrap().to_str().unwrap(),
            "specific failure"
        );
        let detail = exception.get_details().unwrap().get(0);
        assert_eq!(detail.get_detail_id(), 0x1234);
        assert_eq!(detail.get_data().unwrap(), [7, 8, 9]);
        let n = window.bytes().len();
        window.advance(n).unwrap();
        driver.release_call(call, messages);
        driver.runtime.release(target);
        hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
        close(driver, messages, &mut queue);
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
fn synchronous_protected_method_completes_without_an_executor_task() {
    let completed = Rc::new(Cell::new(false));
    let marker = completed.clone();
    let service = server(move |results| {
        let marker = marker.clone();
        capability::DispatchCallResult::with_cancellation_policy(
            Promise::from_future(async move {
                marker.set(true);
                drop(results);
                Ok(())
            }),
            false,
            false,
        )
    });
    let results = Results::new();
    let executor = Rc::new(Executor::default());
    let mut execution = prepared()
        .execute(&*service.hook, &results, Some(executor.clone()))
        .unwrap();
    assert!(matches!(poll(&mut execution), Poll::Ready(Ok(()))));
    assert!(completed.get());
    assert!(executor.0.borrow().is_empty());
}
#[test]
fn cancellation_keeps_non_cancellable_execution_and_result_context_alive() {
    for cancellable in [false, true] {
        let dropped = Rc::new(Cell::new(false));
        let completed = Rc::new(Cell::new(false));
        struct Marker(Rc<Cell<bool>>);
        impl Drop for Marker {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let dropped2 = dropped.clone();
        let completed2 = completed.clone();
        let (sender, receiver) = shared::channel::<()>();
        let receiver = Rc::new(RefCell::new(Some(receiver)));
        let service = server(move |mut r| {
            let ready = receiver.borrow_mut().take().unwrap();
            let marker = Marker(dropped2.clone());
            let completed = completed2.clone();
            r.get()
                .set_as::<capnp::text::Owned>("partial result")
                .unwrap();
            // Drop Results before the application completes. Its cancellation
            // guard must retain those bytes with the independent executor task.
            drop(r);
            capability::DispatchCallResult::with_cancellation_policy(
                Promise::from_future(async move {
                    let _marker = marker;
                    ready.await.map_err(crate::canceled_to_error)?;
                    completed.set(true);
                    Ok(())
                }),
                false,
                cancellable,
            )
        });
        let results = Results::new();
        let weak = Rc::downgrade(&results.0);
        let executor = Rc::new(Executor::default());
        let mut execution = prepared()
            .execute(&*service.hook, &results, Some(executor.clone()))
            .unwrap();
        assert!(poll(&mut execution).is_pending());
        drop((execution, results));
        assert_eq!(dropped.get(), cancellable);
        if cancellable {
            assert!(executor.0.borrow().is_empty());
        } else {
            assert!(weak.upgrade().unwrap().payload.borrow().is_some());
            assert!(sender.send(()).is_ok());
            let mut task = executor.0.borrow_mut().pop().unwrap();
            assert!(matches!(poll(&mut task), Poll::Ready(Ok(()))));
            drop(task);
            assert!(completed.get());
            assert!(dropped.get());
        }
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn tail_forwarding_preserves_child_results() {
    let child = server(|mut results| {
        results
            .get()
            .set_as::<capnp::text::Owned>("tail result")
            .unwrap();
        capability::DispatchCallResult::new(Promise::ok(()), false)
    });
    let parent = server(move |results| {
        capability::DispatchCallResult::new(
            results.hook.tail_call(child.hook.new_call(7, 9, None).hook),
            false,
        )
    });
    let results = Results::new();
    let mut execution = prepared().execute(&*parent.hook, &results, None).unwrap();
    assert!(matches!(poll(&mut execution), Poll::Ready(Ok(()))));
    assert_eq!(
        results
            .0
            .payload
            .borrow()
            .as_ref()
            .unwrap()
            .reader()
            .unwrap()
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "tail result"
    );
}
