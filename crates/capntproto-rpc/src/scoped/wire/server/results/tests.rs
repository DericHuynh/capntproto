use super::*;

#[test]
fn protected_context_borrows_only_with_executor_and_immediate_permission() {
    use capnp::private::capability::CallContextHook;
    struct Tracked {
        inner: InboundCall,
        takes: Rc<Cell<usize>>,
    }
    impl CallContextHook for Tracked {
        fn borrow(&mut self) -> (&dyn ParamsHook, &mut dyn ResultsHook) {
            self.inner.borrow()
        }
        fn results(&self) -> &dyn ResultsHook {
            self.inner.results()
        }
        fn take(&mut self) -> (Box<dyn ParamsHook>, Box<dyn ResultsHook>) {
            self.takes.set(self.takes.get() + 1);
            self.inner.take()
        }
    }
    for executor in [false, true] {
        for streaming in [false, true] {
            let takes = Rc::new(Cell::new(0));
            let called = Rc::new(Cell::new(false));
            let mut tracked = Tracked {
                inner: InboundCall {
                    params: Some(prepared().params),
                    results: Some(Hook {
                        results: Results::new(),
                        message: None,
                        caps: vec![],
                        executor: executor.then(|| {
                            Rc::new(Executor::default()) as Rc<dyn capability::CallExecutor>
                        }),
                    }),
                },
                takes: takes.clone(),
            };
            let context = capability::CallContext::<any_pointer::Owned, any_pointer::Owned>::new(
                &mut tracked,
            );
            let seen = called.clone();
            let method =
                move |_: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>| {
                    seen.set(true);
                    Promise::ok(())
                };
            // Generated streaming methods always defer until their ordering
            // barrier is installed. Non-streaming protected methods use the
            // same permission as the runtime's existing immediate first poll.
            let promise = if streaming {
                context.defer(method)
            } else {
                context.dispatch_protected(method)
            };
            let immediate = executor && !streaming;
            assert_eq!(called.get(), immediate);
            assert_eq!(takes.get(), usize::from(!immediate));
            futures::executor::block_on(promise).unwrap();
            assert!(called.get());
        }
    }
}

#[test]
fn call_context_preserves_borrowed_and_suspended_results() {
    #[derive(Clone)]
    struct Native(Rc<Cell<usize>>, Rc<Cell<usize>>);
    impl capability::Server for Native {
        fn dispatch_call(
            self,
            _: u64,
            method: u16,
            mut context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
        ) -> capability::DispatchCallResult {
            let promise = if method == 0 {
                self.0.set(self.0.get() + 1);
                Promise::from(context.get().map(|(_, mut root)| {
                    root.set_as_capability(crate::broken::new_cap(Error::failed(
                        "borrowed result".into(),
                    )));
                }))
            } else {
                self.1.set(self.1.get() + 1);
                let (_params, results) = context.into_owned();
                Promise::from_future(async move {
                    drop(results);
                    Ok(())
                })
            };
            capability::DispatchCallResult::new(promise, false)
        }
        fn as_ptr(&self) -> usize {
            Rc::as_ptr(&self.0) as usize
        }
    }
    let native = Rc::new(Cell::new(0));
    let owned = Rc::new(Cell::new(0));
    let client = crate::local::Client::new(Native(native.clone(), owned.clone()));
    for (method, immediate) in [(0, true), (1, true), (0, false)] {
        let results = Results::new();
        let prepared = Prepared {
            method: DynamicCall {
                method,
                ..Default::default()
            },
            params: Payload::owned(Default::default(), vec![])
                .into_params()
                .into(),
        };
        let work = prepared
            .execute_using(&client, &results, None, immediate)
            .unwrap();
        assert!(results.pipeline(pipeline::Path::new(&[])).is_none());
        futures::executor::block_on(work).unwrap();
        if method == 0 && immediate {
            let retained = results.pipeline(pipeline::Path::new(&[])).unwrap();
            drop(results);
            assert_eq!(retained.get_failure().unwrap().extra, "borrowed result");
        }
    }
    assert_eq!(native.get(), 2);
    assert_eq!(owned.get(), 1);
}

#[test]
fn pipeline_paths_preserve_early_precedence_and_retained_result_capabilities() {
    #[derive(Clone)]
    struct Early(Rc<RefCell<Vec<Vec<u16>>>>);
    impl PipelineHook for Early {
        fn add_ref(&self) -> Box<dyn PipelineHook> {
            Box::new(self.clone())
        }
        fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
            self.0.borrow_mut().push(
                ops.iter()
                    .map(|op| match op {
                        PipelineOp::GetPointerField(field) => *field,
                        PipelineOp::Noop => panic!("normalized path has no no-op"),
                    })
                    .collect(),
            );
            crate::broken::new_cap(Error::failed("early result".into()))
        }
    }
    for depth in [0, 1, 8, 9, 32] {
        let fields = vec![0; depth];
        let path = pipeline::Path::new(&fields);
        let results = Results::new();
        assert!(results.pipeline(path).is_none());
        let mut hook = Hook {
            results: results.clone(),
            message: None,
            caps: vec![],
            executor: None,
        };
        let mut pointer = hook.get_with_size_hint(None).unwrap();
        for _ in 0..depth {
            pointer = pointer
                .init_as::<rpc_capnp::payload::Builder<'_>>()
                .get_content();
        }
        pointer.set_as_capability(crate::broken::new_cap(Error::failed("final result".into())));
        drop(hook);
        assert!(
            results.pipeline(path).is_none(),
            "returned builder still waits for execution"
        );
        results.0.complete(Ok(()));
        let retained = results.pipeline(path).unwrap();
        assert_eq!(retained.get_failure().unwrap().extra, "final result");
        let invalid = vec![u16::MAX; depth + 1];
        assert!(results
            .pipeline(pipeline::Path::new(&invalid))
            .unwrap()
            .get_failure()
            .is_some());
        results.fail(Error::failed("method failed".into()));
        assert_eq!(
            results.pipeline(path).unwrap().get_failure().unwrap().extra,
            "method failed"
        );
        let observed = Rc::new(RefCell::new(Vec::new()));
        *results.0.pipeline.borrow_mut() = Some(Rc::new(Early(observed.clone())));
        let early_fields: Vec<_> = (0..depth as u16).collect();
        assert_eq!(
            results
                .pipeline(pipeline::Path::new(&early_fields))
                .unwrap()
                .get_failure()
                .unwrap()
                .extra,
            "early result"
        );
        assert_eq!(*observed.borrow(), vec![early_fields]);
        drop(results);
        assert_eq!(retained.get_failure().unwrap().extra, "final result");
    }
}

#[test]
fn cached_results_clear_state_and_never_reuse_retained_owners() {
    let cache = Cache::default();
    let first = cache.acquire(None, None);
    let retained = first.clone();
    first.0.method.set(Some(DynamicCall::default()));
    first.0.complete(Err(Error::failed("old result".into())));
    cache.recycle(first);
    let second = cache.acquire(None, None);
    assert!(!Rc::ptr_eq(&second.0, &retained.0));
    assert!(second.0.method.get().is_none());
    assert!(matches!(*second.0.completion.borrow(), Completion::Running));
    assert_eq!(retained.failure().unwrap().extra, "old result");
    drop(retained);
    let pointer = Rc::as_ptr(&second.0);
    second.0.method.set(Some(DynamicCall::default()));
    second.0.transferred.set(true);
    second.0.trace_prepared.set(true);
    *second.0.trace.borrow_mut() = Some("old trace".into());
    second.0.complete(Ok(()));
    cache.recycle(second);
    let third = cache.acquire(None, None);
    assert_eq!(Rc::as_ptr(&third.0), pointer);
    assert!(third.0.method.get().is_none());
    assert!(matches!(*third.0.completion.borrow(), Completion::Running));
    assert!(!third.0.transferred.get());
    assert!(!third.0.trace_prepared.get());
    assert!(third.0.trace.borrow().is_none());
}

#[test]
fn shared_cancellation_guard_retains_context_and_prevents_cache_reuse() {
    struct Lifetime(Rc<Cell<bool>>);
    impl Drop for Lifetime {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let cache = Cache::default();
    let dropped = Rc::new(Cell::new(false));
    let results = cache.acquire(None, None);
    *results.0.context.borrow_mut() = Some(Rc::new(Lifetime(dropped.clone())));
    let address = Rc::as_ptr(&results.0);
    let hook = Hook {
        results: results.clone(),
        message: None,
        caps: vec![],
        executor: None,
    };
    let guard = hook
        .cancellation_context()
        .expect("protected execution owner");
    let capnp::private::capability::CancellationGuard::Shared(owner) = &guard else {
        panic!("results share their existing context allocation");
    };
    assert_eq!(
        owner.as_ref().downcast_ref::<State>().unwrap() as *const State,
        address
    );
    drop(hook);
    cache.recycle(results);
    assert!(!dropped.get());
    let next = cache.acquire(None, None);
    assert_ne!(Rc::as_ptr(&next.0), address);
    drop(guard);
    assert!(dropped.get());
}

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
        context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        let (_, results) = context.into_owned();
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
        params: Payload::owned(Default::default(), vec![])
            .into_params()
            .into(),
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
        assert!(results.0.payload.get().is_none());
        wake.0.store(0, Ordering::SeqCst);
        assert_eq!(
            results.poll_work(&mut execution, &mut cx).is_pending(),
            pending
        );
        assert!(results.0.payload.get().is_some());
        assert_eq!(wake.0.load(Ordering::SeqCst), 0);
        if pending {
            results.fail(Error::failed("external completion".into()));
            assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        } else {
            assert!(results.0.wake.borrow().is_empty());
            assert_eq!(results.0.wake.borrow().overflow.capacity(), 0);
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
    assert!(matches!(
        *results.0.completion.borrow(),
        Completion::AwaitingPayload
    ));
    assert!(results.0.payload.get().is_none());
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
            let payload = results.0.payload.get().unwrap().share(&results);
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
    let source = source.freeze();
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, Results::new());
        assert!(driver
            .local_answer_prepared(call.answer(), 7, 9, &source, messages, Default::default())
            .is_err());
        assert!(driver.prepare_answer_return(call.answer()).is_ok());
        driver.release_call(call, messages);
        driver.capabilities().release(target);
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        close(driver, messages, &mut queue);
    });
}

#[test]
fn prepared_capability_results_preserve_indices_limits_and_retry_ownership() {
    for first_segment_words in [8, 64] {
        let mut source = connection::PreparedReturn::from_template(
            &connection::PreparedReturnTemplate::default(),
            capnp::message::HeapAllocator::new().first_segment_words(first_segment_words),
        );
        // Keep index zero unused; the copied capability must remain index one.
        let mut caps = vec![None];
        let mut root = source.results();
        root.imbue_mut(&mut caps);
        let mut values = root.initn_as::<capnp::any_pointer_list::Builder>(2);
        values
            .reborrow()
            .get(0)
            .set_as_capability(crate::broken::new_cap(Error::failed(
                "source hook is never invoked by wire copying".into(),
            )));
        values
            .get(1)
            .set_as::<capnp::data::Owned>(&b"retained bytes"[..])
            .unwrap();
        let source = source.freeze();
        with_driver(|mut driver, messages| {
            let (call, target) = request(&mut driver, messages, Results::new());
            let value = driver
                .capabilities()
                .endpoint(Route::local(73), clients::Location::Local)
                .unwrap();
            for references in [
                &[Some(&value)][..],
                &[None, Some(&value), None, None, None][..],
            ] {
                assert!(driver
                    .local_answer_prepared_with_capabilities(
                        call.answer(),
                        7,
                        9,
                        &source,
                        references.iter().copied(),
                        messages,
                        Default::default(),
                    )
                    .is_err());
            }
            let references = [None, Some(&value)];
            assert!(driver
                .local_answer_prepared_with_capabilities(
                    call.answer(),
                    7,
                    9,
                    &source,
                    references.iter().copied(),
                    messages,
                    capnp::message::ReaderOptions {
                        nesting_limit: 0,
                        ..Default::default()
                    },
                )
                .is_err());
            assert!(driver
                .local_answer_prepared_with_capabilities(
                    call.answer(),
                    7,
                    9,
                    &source,
                    references.iter().copied(),
                    messages,
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: Some(1),
                        ..Default::default()
                    },
                )
                .unwrap()
                .is_none());
            let mut reserved = vec![];
            while let Ok(writer) = messages.reserve(4096) {
                reserved.push(writer);
            }
            assert!(driver
                .local_answer_prepared_with_capabilities(
                    call.answer(),
                    7,
                    9,
                    &source,
                    references.iter().copied(),
                    messages,
                    Default::default(),
                )
                .is_err());
            for writer in reserved {
                messages.discard(writer);
            }
            for _ in 0..3 {
                let result = driver
                    .local_answer_prepared_with_capabilities(
                        call.answer(),
                        7,
                        9,
                        &source,
                        references.iter().copied(),
                        messages,
                        Default::default(),
                    )
                    .unwrap()
                    .unwrap();
                result
                    .with(messages, |payload| {
                        assert_eq!(payload.capabilities().len(), 2);
                        assert!(payload.capabilities()[0].is_none());
                        assert!(payload.capabilities()[1].is_some());
                        let values = payload
                            .content()
                            .get_as::<capnp::any_pointer_list::Reader>()
                            .unwrap();
                        assert_eq!(values.get(0).capability_index().unwrap(), 1);
                        assert_eq!(
                            values.get(1).get_as::<capnp::data::Reader>().unwrap(),
                            b"retained bytes"
                        );
                    })
                    .unwrap();
                result.release(messages, &mut driver.capabilities());
                assert!(driver.prepare_answer_return(call.answer()).is_ok());
            }
            // A successful publication under two capability slots does not
            // authorize the same bytes against a smaller table on a later retry.
            assert!(driver
                .local_answer_prepared_with_capabilities(
                    call.answer(),
                    7,
                    9,
                    &source,
                    [Some(&value)].into_iter(),
                    messages,
                    Default::default(),
                )
                .is_err());
            assert!(driver
                .local_answer_prepared_with_capabilities(
                    call.answer(),
                    7,
                    9,
                    &source,
                    references.iter().copied(),
                    messages,
                    capnp::message::ReaderOptions {
                        nesting_limit: 0,
                        ..Default::default()
                    },
                )
                .is_err());
            driver.capabilities().release(value);
            driver.capabilities().release(target);
            driver.release_call(call, messages);
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            close(driver, messages, &mut queue);
            let mut reserved = vec![];
            while let Ok(writer) = messages.reserve(4096) {
                reserved.push(writer);
            }
            assert_eq!(reserved.len(), 12, "every temporary frame was reclaimed");
            for writer in reserved {
                messages.discard(writer);
            }
        });
    }
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
        .capabilities()
        .endpoint(Route::local(31), clients::Location::Local)
        .unwrap();
    let id = {
        let (mut graph, mut batch) = driver.export_batch();
        let id = capntproto_rpc_wire::clients::stage(&mut graph, &mut batch, &target)
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
    let call = driver
        .admit_request(
            frame,
            messages,
            connection::Answer::new(pipelines::Results::new(results)),
            |_| (),
        )
        .unwrap_or_else(|_| panic!("admit"));
    assert!(driver.answers().dispatch(call.answer(), true).unwrap());
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
    check_hosted_reply(false);
    check_hosted_reply(true);
}
fn check_hosted_reply(use_prepared: bool) {
    with_driver(|mut driver, messages| {
        let results = Results::new();
        let (call, target) = request(&mut driver, messages, results.clone());
        let value = driver
            .capabilities()
            .endpoint(Route::local(73), clients::Location::Local)
            .unwrap();
        let mut hooks = ClientHooks::new(16, 16);
        let cap = hooks
            .export_client(&mut driver.capabilities(), &value)
            .unwrap();
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
        let parent = driver.answer_pipeline(call.answer().id()).unwrap();
        let observed = driver
            .project_answer(&parent, pipeline::Path::new(&[]))
            .unwrap();
        driver.release_answer_pipeline_deferred(parent);
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
            .collect_server_output(
                &results,
                &mut driver,
                messages,
                Default::default(),
                use_prepared.then_some(call.answer()),
                |_, _| panic!("foreign")
            )
            .is_err());
        for writer in reserved {
            messages.discard(writer);
        }
        let output = hooks
            .collect_server_output(
                &results,
                &mut driver,
                messages,
                Default::default(),
                use_prepared.then_some(call.answer()),
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
        driver.capabilities().release(observed);
        driver.capabilities().release(value);
        driver.capabilities().release(target);
        driver.release_call(call, messages);
        drop((results, execution, service));
        hooks.close(
            &mut driver.capabilities(),
            Error::disconnected("done".into()),
        );
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
        driver.capabilities().release(target);
        hooks.close(
            &mut driver.capabilities(),
            Error::disconnected("done".into()),
        );
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
            assert!(weak.upgrade().unwrap().payload.get().is_some());
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
            .get()
            .unwrap()
            .reader()
            .unwrap()
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "tail result"
    );
}

struct PipelineTailRequest {
    sent: Rc<Cell<bool>>,
    dropped: Rc<Cell<bool>>,
}
impl Drop for PipelineTailRequest {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}
impl RequestHook for PipelineTailRequest {
    fn get(&mut self) -> any_pointer::Builder<'_> {
        panic!("tail forwarding must not rebuild the request")
    }
    fn get_brand(&self) -> usize {
        panic!("pipeline-only or rejected tail call must not inspect the target")
    }
    fn send(self: Box<Self>) -> capability::RemotePromise<any_pointer::Owned> {
        panic!("pipeline-only or rejected tail call must not send an ordinary request")
    }
    fn send_streaming(self: Box<Self>) -> Promise<(), Error> {
        panic!("tail call must not send a streaming request")
    }
    fn tail_send(self: Box<Self>) -> Option<(u32, Promise<(), Error>, Box<dyn PipelineHook>)> {
        panic!("pipeline-only or rejected tail call must not transfer results")
    }
    fn send_for_pipeline(self: Box<Self>) -> any_pointer::Pipeline {
        self.sent.set(true);
        any_pointer::Pipeline::new(Box::new(crate::broken::Pipeline::new(Error::failed(
            "child pipeline".into(),
        ))))
    }
}

#[test]
fn tail_call_rejects_initialized_results_before_sending() {
    for only_promise_pipeline in [false, true] {
        for populated in [false, true] {
            let results = Results::new();
            results.0.method.set(Some(DynamicCall {
                only_promise_pipeline,
                ..Default::default()
            }));
            let mut hook = Box::new(Hook {
                results: results.clone(),
                message: None,
                caps: vec![],
                executor: None,
            });
            let mut root = hook.get().unwrap();
            if populated {
                root.set_as::<capnp::text::Owned>("already initialized")
                    .unwrap();
            }
            let sent = Rc::new(Cell::new(false));
            let dropped = Rc::new(Cell::new(false));
            let (completion, pipeline) = hook.direct_tail_call(Box::new(PipelineTailRequest {
                sent: sent.clone(),
                dropped: dropped.clone(),
            }));
            let error = futures::executor::block_on(completion).unwrap_err();
            assert_eq!(error.extra, "tail_call after initializing results");
            assert_eq!(
                pipeline.get_pipelined_cap(&[]).get_failure().unwrap().extra,
                error.extra
            );
            assert!(!sent.get());
            assert!(dropped.get());
            assert!(results.0.payload.get().is_some());
        }
    }
}

#[test]
fn pipeline_only_tail_call_preserves_published_pipeline_and_context_until_cancel() {
    for early_pipeline in [false, true] {
        for poll_before_cancel in [false, true] {
            let results = Results::new();
            results.0.method.set(Some(DynamicCall {
                only_promise_pipeline: true,
                ..Default::default()
            }));
            let mut hook = Box::new(Hook {
                results: results.clone(),
                message: None,
                caps: vec![],
                executor: None,
            });
            if early_pipeline {
                hook.set_pipeline_from(Box::new(crate::broken::Pipeline::new(Error::failed(
                    "early pipeline".into(),
                ))))
                .unwrap();
            }
            let sent = Rc::new(Cell::new(false));
            let dropped = Rc::new(Cell::new(false));
            let (mut completion, pipeline) = hook.direct_tail_call(Box::new(PipelineTailRequest {
                sent: sent.clone(),
                dropped: dropped.clone(),
            }));
            assert!(sent.get());
            assert!(dropped.get());
            assert_eq!(
                pipeline.get_pipelined_cap(&[]).get_failure().unwrap().extra,
                "child pipeline"
            );
            assert_eq!(
                results
                    .0
                    .pipeline
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .get_pipelined_cap(&[])
                    .get_failure()
                    .unwrap()
                    .extra,
                if early_pipeline {
                    "early pipeline"
                } else {
                    "child pipeline"
                }
            );
            assert!(results.0.payload.get().is_none());
            if poll_before_cancel {
                assert!(poll(&mut completion).is_pending());
                assert!(results.0.payload.get().is_none());
            }
            drop(completion);
            assert!(results.0.payload.get().is_some());
        }
    }
}

#[test]
fn only_affine_protocol_retirement_notifies_running_worker() {
    let queue = super::super::super::ready::Queue::default();
    let waker = queue.insert(0, true);
    let results = Results::new();
    results.bind_protocol_waker(waker);
    assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
    let producer = results.clone().into_producer();
    drop(results.clone());
    assert_eq!(queue.pop().map(|ready| ready.index()), None);
    drop(producer);
    assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
    results.unbind_protocol_waker();
    drop(results.into_producer());
    assert_eq!(queue.pop().map(|ready| ready.index()), None);
}

#[test]
fn retained_results_preserve_payloads_without_blocking_segmented_worker_reuse() {
    for segmented in [false, true] {
        let cache = Cache::default();
        let results = cache.acquire(None, None);
        let address = Rc::as_ptr(&results.0);
        let mut message = connection::PreparedReturn::from_template(
            &connection::PreparedReturnTemplate::default(),
            capnp::message::HeapAllocator::new().first_segment_words(16),
        );
        let bytes = vec![0xa5; if segmented { 8192 } else { 8 }];
        message
            .results()
            .set_as::<capnp::data::Owned>(bytes.as_slice())
            .unwrap();
        drop(Hook {
            results: results.clone(),
            message: Some(message),
            caps: vec![],
            executor: None,
        });
        results.0.complete(Ok(()));
        let retained = results.0.payload.get().unwrap().share(&results);
        cache.recycle(results);
        let next = cache.acquire(None, None);
        assert_eq!(Rc::as_ptr(&next.0) == address, segmented,
            "segmented output retains its message, while inline output retains its producing context");
        assert_eq!(
            retained
                .reader()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            bytes
        );
        assert!(next.0.payload.get().is_none());
    }
}

#[test]
fn suspended_results_keep_one_waiter_inline_and_notify_distinct_observers() {
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
    let results = Results::new();
    let counts = [Arc::new(Count::default()), Arc::new(Count::default())];
    let wakes = counts.each_ref().map(|count| Waker::from(count.clone()));
    let first = allocation_counter::measure(|| {
        for _ in 0..3 {
            assert!(results
                .poll_completed(&mut Context::from_waker(&wakes[0]))
                .is_pending());
        }
    });
    assert_eq!(first.count_total, 0);
    assert!(results
        .poll_completed(&mut Context::from_waker(&wakes[1]))
        .is_pending());
    results.0.complete(Err(Error::failed("complete".into())));
    for count in counts {
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }
    assert!(results.0.wake.borrow().is_empty());
}

#[test]
fn owned_completion_notifies_once_when_execution_and_payload_are_both_ready() {
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
    for execution_first in [false, true] {
        let owner = Arc::new(Count::default());
        let observer = Arc::new(Count::default());
        let results = Results::new();
        results.bind_protocol_waker(Waker::from(owner.clone()));
        let observer_wake = Waker::from(observer.clone());
        assert!(results
            .poll_completed(&mut Context::from_waker(&observer_wake))
            .is_pending());
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
        if execution_first {
            assert!(matches!(poll(&mut execution), Poll::Ready(Ok(()))));
            assert!(
                matches!(poll(&mut execution), Poll::Ready(Ok(()))),
                "execution can be observed again while its builder is retained"
            );
            assert!(results.poll_owned_completed().is_pending());
            assert_eq!(owner.0.load(Ordering::SeqCst), 0);
            assert_eq!(observer.0.load(Ordering::SeqCst), 0);
            drop(hook);
        } else {
            drop(hook);
            assert!(results.poll_owned_completed().is_pending());
            assert_eq!(owner.0.load(Ordering::SeqCst), 0);
            assert_eq!(observer.0.load(Ordering::SeqCst), 0);
            assert!(matches!(poll(&mut execution), Poll::Ready(Ok(()))));
        }
        assert!(matches!(
            results.poll_owned_completed(),
            Poll::Ready(Ok(()))
        ));
        assert_eq!(owner.0.load(Ordering::SeqCst), 1);
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        assert!(results.0.wake.borrow().is_empty());
        results.unbind_protocol_waker();
    }
}

#[test]
fn owned_pending_execution_uses_no_completion_waiter_and_keeps_early_pipeline_notifications() {
    let queue = super::super::super::ready::Queue::default();
    let results = Results::new();
    results.bind_protocol_waker(queue.insert(0, true));
    assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
    let mut execution = Execution {
        operation: Some(Promise::from_future(core::future::pending())),
        results: results.clone(),
    };
    assert!(poll(&mut execution).is_pending());
    assert!(results.0.wake.borrow().is_empty());
    let mut hook = Hook {
        results: results.clone(),
        message: None,
        caps: vec![],
        executor: None,
    };
    hook.set_pipeline().unwrap();
    assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
    assert!(results.early_pipeline().is_some());
    drop(hook);
    assert_eq!(
        queue.pop().map(|ready| ready.index()),
        None,
        "returning a builder cannot make an unfinished execution collectible"
    );
    results.fail(Error::failed("failed while suspended".into()));
    assert_eq!(queue.pop().map(|ready| ready.index()), Some(0));
    assert!(
        matches!(results.poll_owned_completed(), Poll::Ready(Err(error)) if error.extra == "failed while suspended")
    );
    assert!(results.0.wake.borrow().is_empty());
    results.unbind_protocol_waker();
}

#[test]
fn result_pool_reuses_concurrent_contexts_without_allocating_after_warmup() {
    for maximum in [0, 1, 2, 8, 32, 65] {
        let cache = Cache::new(maximum);
        let retained = maximum.min(64);
        let mut live = Vec::with_capacity(maximum.max(1));
        for _ in 0..maximum.max(1) {
            live.push(cache.acquire(None, None));
        }
        let addresses: Vec<_> = live
            .iter()
            .take(retained)
            .map(|r| Rc::as_ptr(&r.0))
            .collect();
        for result in live.drain(..) {
            cache.recycle(result);
        }
        let first = cache.first.take();
        assert_eq!(
            usize::from(first.is_some()) + cache.overflow.borrow().len(),
            retained
        );
        cache.first.set(first);
        let measured = allocation_counter::measure(|| {
            for _ in 0..3 {
                for _ in 0..retained {
                    let result = cache.acquire(None, None);
                    assert!(addresses.contains(&Rc::as_ptr(&result.0)));
                    assert!(matches!(*result.0.completion.borrow(), Completion::Running));
                    assert!(result.0.payload.get().is_none());
                    assert!(result.0.wake.borrow().is_empty());
                    result
                        .0
                        .complete(Err(Error::from_kind(capnp::ErrorKind::Failed)));
                    live.push(result);
                }
                for result in live.drain(..) {
                    cache.recycle(result);
                }
            }
        });
        assert_eq!(measured.count_total, 0, "warmed concurrency {maximum}");
    }
}

#[test]
fn result_pool_resets_context_before_reentrant_admission() {
    struct Reenter {
        cache: Rc<Cache>,
        dropped: Rc<Cell<bool>>,
    }
    impl Drop for Reenter {
        fn drop(&mut self) {
            let result = self.cache.acquire(None, None);
            self.cache.recycle(result);
            self.dropped.set(true);
        }
    }
    let cache = Rc::new(Cache::new(2));
    let dropped = Rc::new(Cell::new(false));
    let result = cache.acquire(None, None);
    *result.0.context.borrow_mut() = Some(Rc::new(Reenter {
        cache: cache.clone(),
        dropped: dropped.clone(),
    }));
    cache.recycle(result);
    assert!(dropped.get());
    let first = cache.acquire(None, None);
    let second = cache.acquire(None, None);
    assert!(!Rc::ptr_eq(&first.0, &second.0));
    assert!(first.0.context.borrow().is_none());
    assert!(second.0.context.borrow().is_none());
}

#[test]
fn ready_execution_notifies_observers_without_waking_its_active_owner() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl futures::task::ArcWake for Count {
        fn wake_by_ref(this: &Arc<Self>) {
            this.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    for (failed, active_owner) in [(false, true), (true, true), (false, false), (true, false)] {
        let owner = Arc::new(Count::default());
        let observer = Arc::new(Count::default());
        // Use a shared-vtable factory to exercise exact identity. std::task::Wake
        // can give cloned wakers distinct vtable addresses under Miri;
        // will_wake() may then return false, requiring the safe extra wake.
        let owner_wake = futures::task::waker(owner.clone());
        let observer_wake = futures::task::waker(observer.clone());
        let results = Results::new();
        results.bind_protocol_waker(owner_wake.clone());
        assert!(results
            .0
            .protocol_wake
            .borrow()
            .as_ref()
            .unwrap()
            .will_wake(&owner_wake));
        let mut hook = Some(Hook {
            results: results.clone(),
            message: None,
            caps: vec![],
            executor: None,
        });
        if !failed {
            drop(hook.take());
        }
        assert!(results
            .poll_completed(&mut Context::from_waker(&observer_wake))
            .is_pending());
        let mut execution = Execution {
            operation: Some(if failed {
                Promise::err(Error::failed("service failed".into()))
            } else {
                Promise::ok(())
            }),
            results: results.clone(),
        };
        let current = if active_owner {
            &owner_wake
        } else {
            &observer_wake
        };
        let Poll::Ready(result) = Pin::new(&mut execution).poll(&mut Context::from_waker(current))
        else {
            panic!("ready service must complete in its active owner's poll");
        };
        assert_eq!(result.is_err(), failed);
        assert_eq!(
            owner.0.load(Ordering::SeqCst),
            usize::from(!active_owner),
            "only the owner already observing completion may skip its wake"
        );
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        assert!(results.poll_owned_completed().is_ready());
        drop(hook);
        assert_eq!(owner.0.load(Ordering::SeqCst), usize::from(!active_owner));
        assert_eq!(observer.0.load(Ordering::SeqCst), 1);
        results.unbind_protocol_waker();
    }
}

#[test]
fn native_tail_context_accepts_only_its_live_inbox_and_does_not_retain_owners() {
    let a = ClientHooks::new(1, 1);
    let b = ClientHooks::new(1, 1);
    let results = Results::new();
    let tail = NativeTail {
        origin: Rc::downgrade(&a.inbox),
        owner: Rc::downgrade(&results.0),
        phase: Cell::new(TailPhase::Prepared),
    };
    assert!(tail.accepts(&Rc::downgrade(&a.inbox)));
    assert!(!tail.accepts(&Rc::downgrade(&b.inbox)));
    let forwarded = NativeTail {
        origin: Rc::downgrade(&a.inbox),
        owner: Rc::downgrade(&results.0),
        phase: Cell::new(TailPhase::Prepared),
    };
    assert!(forwarded.pending());
    forwarded.forwarded();
    assert!(!forwarded.pending());
    assert!(!forwarded.remote());
    assert!(!tail.remote());
    tail.published(false);
    assert!(tail.remote());
    tail.forwarded();
    assert!(
        tail.pending(),
        "completion cannot downgrade an accepted remote transfer"
    );

    assert!(!tail.sent());
    drop(results);
    assert!(tail.owner.upgrade().is_none());
    tail.published(true);
    assert!(tail.sent());
    drop(a);
    assert!(!tail.accepts(&tail.origin));
    assert!(!tail.accepts(&Rc::downgrade(&b.inbox)));
}
