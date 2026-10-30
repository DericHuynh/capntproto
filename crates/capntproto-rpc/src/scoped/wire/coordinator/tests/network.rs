use super::super::network::Network;
use super::*;

fn coordinator<'p, 'id, 's>(driver: TestPeer<'p, 'id, 's>) -> TestCoordinator<'p, 'id, 's> {
    Coordinator::new(
        driver,
        ClientHooks::new(32, 16),
        NonZeroUsize::new(4).unwrap(),
        None,
        options(),
    )
    .unwrap()
}
fn poll<T>(future: &mut (impl Future<Output = T> + Unpin)) -> Poll<T> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}
fn closed(handle: &Handle) {
    let client: capability::Client = handle.bootstrap();
    assert!(
        matches!(poll(&mut client.when_resolved()), Poll::Ready(Err(error))
        if error.kind == capnp::ErrorKind::Disconnected)
    );
}

#[test]
fn network_shutdown_waits_for_flush_close_and_cleanup_and_wakes_all_observers() {
    with_peer(|mut driver, messages| {
        let network = Network::new();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let expected = super::lifecycle::queued(&mut driver, messages, &mut queue);
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let handle = network.connection(1).unwrap();
        let pending: capability::Client = handle.bootstrap();
        let input = Rc::new(RefCell::new(Pipe::default()));
        let output = Rc::new(RefCell::new(Pipe {
            block_close: true,
            ..Default::default()
        }));
        let (mut reader, mut writer) = (Reader(input.clone()), Writer(output.clone()));
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let (mut first, mut second) = (network.get_disconnector(), network.get_disconnector());
        let (a, b) = (
            Arc::new(WakeCount::default()),
            Arc::new(WakeCount::default()),
        );
        let (aw, bw) = (Waker::from(a.clone()), Waker::from(b.clone()));
        assert!(Pin::new(&mut first)
            .poll(&mut Context::from_waker(&aw))
            .is_pending());
        assert!(Pin::new(&mut second)
            .poll(&mut Context::from_waker(&bw))
            .is_pending());
        assert!(network.connection(1).is_none());
        closed(&handle);
        assert!(matches!(
            poll(&mut pending.when_resolved()),
            Poll::Ready(Err(_))
        ));
        {
            let mut run = std::pin::pin!(registered.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::pending(),
                std::future::pending()
            ));
            for _ in 0..2000 {
                assert!(poll(&mut run).is_pending());
                if output.borrow().close_calls > 0 {
                    break;
                }
            }
            assert!(output.borrow().close_calls > 0);
            assert_eq!(
                output.borrow().bytes.iter().copied().collect::<Vec<_>>(),
                expected
            );
            assert!(!output.borrow().closed);
            assert!(Pin::new(&mut first)
                .poll(&mut Context::from_waker(&aw))
                .is_pending());
            assert!(Pin::new(&mut second)
                .poll(&mut Context::from_waker(&bw))
                .is_pending());
            let (before_a, before_b) = (a.0.load(Ordering::SeqCst), b.0.load(Ordering::SeqCst));
            output.borrow_mut().block_close = false;
            let mut finished = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = poll(&mut run) {
                    result.unwrap();
                    finished = true;
                    break;
                }
            }
            assert!(finished && output.borrow().closed);
            assert!(a.0.load(Ordering::SeqCst) > before_a && b.0.load(Ordering::SeqCst) > before_b);
        }
        assert!(policy.stopped && queue.is_empty());
        assert!(matches!(poll(&mut first), Poll::Ready(Ok(()))));
        assert!(matches!(poll(&mut second), Poll::Ready(Ok(()))));
        reclaimed(messages);
    });
}

#[test]
fn registered_coordinator_cancellation_before_first_poll_retires_network_owner() {
    with_peer(|driver, messages| {
        let network = Network::new();
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let handle = registered.handle();
        let mut closed = network.get_disconnector();
        let (input, output) = (
            Rc::new(RefCell::new(Pipe::default())),
            Rc::new(RefCell::new(Pipe::default())),
        );
        let (mut reader, mut writer) = (Reader(input.clone()), Writer(output.clone()));
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let run = registered.run_stream(
            messages,
            &mut queue,
            &mut reader,
            &mut writer,
            &mut scratch,
            |_| (),
            &mut policy,
            std::future::pending(),
            std::future::pending(),
        );
        assert!(poll(&mut closed).is_pending());
        drop(run);
        assert!(
            matches!(poll(&mut closed), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected)
        );
        assert!(network.connection(1).is_none());
        self::closed(&handle);
        assert_eq!(input.borrow().read_calls, 0);
        assert_eq!(output.borrow().write_calls + output.borrow().close_calls, 0);
    });
}

#[test]
fn orderly_network_shutdown_drains_output_even_with_no_free_input_buffer() {
    with_peer(|mut driver, messages| {
        let network = Network::new();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let expected = super::lifecycle::queued(&mut driver, messages, &mut queue);
        // One queued frame plus 31 caller-owned buffers exhaust this fixture.
        let held: Vec<_> = (0..31)
            .map(|_| messages.reserve(options().input_bytes.get()).unwrap())
            .collect();
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let (mut reader, mut writer) = (Reader(Rc::default()), Writer(Rc::default()));
        let mut scratch = [0; 32];
        let mut policy = Policy::default();
        let mut disconnected = network.get_disconnector();
        assert!(poll(&mut disconnected).is_pending());
        {
            let mut run = std::pin::pin!(registered.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::pending(),
                std::future::pending()
            ));
            let mut finished = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = poll(&mut run) {
                    result.unwrap();
                    finished = true;
                    break;
                }
            }
            assert!(finished);
        }
        assert_eq!(reader.0.borrow().read_calls, 0);
        assert_eq!(
            writer.0.borrow().bytes.iter().copied().collect::<Vec<_>>(),
            expected
        );
        assert!(writer.0.borrow().closed);
        assert!(matches!(poll(&mut disconnected), Poll::Ready(Ok(()))));
        for buffer in held {
            messages.discard(buffer);
        }
        reclaimed(messages);
    });
}

#[test]
fn detached_coordinator_completion_cannot_remove_reused_network_identity() {
    with_peer(|driver, messages| {
        let network = Network::new();
        let registered = coordinator(driver).register(&network, 7).unwrap();
        let (mut reader, mut writer) = (
            Reader(Rc::default()),
            Writer(Rc::new(RefCell::new(Pipe {
                block_close: true,
                ..Default::default()
            }))),
        );
        let output = writer.0.clone();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        {
            let mut run = std::pin::pin!(registered.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                async { Ok(()) },
                std::future::pending()
            ));
            assert!(poll(&mut run).is_pending());
            assert!(network.connection(7).is_none());
            with_peer(|replacement, _| {
                let replacement = coordinator(replacement).register(&network, 7).unwrap();
                output.borrow_mut().block_close = false;
                let mut finished = false;
                for _ in 0..2000 {
                    if let Poll::Ready(result) = poll(&mut run) {
                        result.unwrap();
                        finished = true;
                        break;
                    }
                }
                assert!(finished && network.connection(7).is_some());
                drop(replacement);
                assert!(network.connection(7).is_none());
            });
        }
        reclaimed(messages);
    });
}

#[test]
fn duplicate_and_closed_registrations_return_coordinator_without_disturbing_live_peer() {
    with_peer(|first, _| {
        with_peer(|second, _| {
            let network = Network::new();
            let first = coordinator(first).register(&network, 1).unwrap();
            let rejected = coordinator(second).register(&network, 1).err().unwrap();
            assert_eq!(rejected.error.kind, capnp::ErrorKind::Overloaded);
            assert!(network.connection(1).is_some());
            let mut disconnected = network.get_disconnector();
            assert!(poll(&mut disconnected).is_pending());
            let rejected = rejected.coordinator.register(&network, 2).err().unwrap();
            assert_eq!(rejected.error.kind, capnp::ErrorKind::Disconnected);
            let other = Network::new();
            let second = rejected.coordinator.register(&other, 2).unwrap();
            drop(first);
            assert!(matches!(poll(&mut disconnected), Poll::Ready(Err(_))));
            assert!(other.connection(2).is_some());
            drop(second);
        })
    });
}

#[test]
fn network_drop_requests_orderly_shutdown_without_retaining_coordinator() {
    with_peer(|driver, messages| {
        let network = Network::new();
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let handle = registered.handle();
        let mut disconnected = network.get_disconnector();
        drop(network);
        closed(&handle);
        let (mut reader, mut writer) = (Reader(Rc::default()), Writer(Rc::default()));
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        {
            let mut run = std::pin::pin!(registered.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::pending(),
                std::future::pending()
            ));
            let mut finished = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = poll(&mut run) {
                    result.unwrap();
                    finished = true;
                    break;
                }
            }
            assert!(finished);
        }
        assert!(writer.0.borrow().closed && policy.stopped);
        assert!(matches!(poll(&mut disconnected), Poll::Ready(Ok(()))));
        reclaimed(messages);
    });
}

#[test]
fn registered_io_failures_reach_network_and_preserve_typed_run_error() {
    for stage in 0..3 {
        with_peer(|mut driver, messages| {
            let network = Network::new();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            super::lifecycle::queued(&mut driver, messages, &mut queue);
            let registered = coordinator(driver).register(&network, 1).unwrap();
            let mut reader = Reader(Rc::new(RefCell::new(Pipe {
                fail_read: stage == 0,
                ..Default::default()
            })));
            let mut writer = Writer(Rc::new(RefCell::new(Pipe {
                fail_write: stage == 1,
                fail_close: stage == 2,
                ..Default::default()
            })));
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            {
                let stop = async {
                    if stage < 2 {
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                };
                let mut run = std::pin::pin!(registered.run_stream(
                    messages,
                    &mut queue,
                    &mut reader,
                    &mut writer,
                    &mut scratch,
                    |_| (),
                    &mut policy,
                    stop,
                    std::future::pending()
                ));
                let mut failure = None;
                for _ in 0..2000 {
                    if let Poll::Ready(result) = poll(&mut run) {
                        failure = Some(result.unwrap_err());
                        break;
                    }
                }
                let error = failure.unwrap();
                assert!(matches!(
                    (stage, error.cause),
                    (0, Some(Cause::Input(_)))
                        | (1, Some(Cause::Output(_)))
                        | (2, Some(Cause::Close("close failed")))
                ));
            }
            assert!(policy.stopped && network.connection(1).is_none());
            assert!(matches!(
                poll(&mut network.get_disconnector()),
                Poll::Ready(Err(_))
            ));
            reclaimed(messages);
        });
    }
}

#[test]
fn caller_abort_wins_network_shutdown_and_reaches_every_observer() {
    with_peer(|driver, messages| {
        let network = Network::new();
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let (mut reader, mut writer) = (Reader(Rc::default()), Writer(Rc::default()));
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let mut disconnected = network.get_disconnector();
        assert!(poll(&mut disconnected).is_pending());
        {
            let mut run = std::pin::pin!(registered.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::pending(),
                async { Error::overloaded("caller deadline".into()) }
            ));
            let mut failed = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = poll(&mut run) {
                    assert!(
                        matches!(result.unwrap_err().cause, Some(Cause::Protocol(error)) if error.extra == "caller deadline")
                    );
                    failed = true;
                    break;
                }
            }
            assert!(failed);
        }
        for observer in [&mut disconnected, &mut network.get_disconnector()] {
            assert!(
                matches!(poll(observer), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Overloaded && error.extra == "caller deadline")
            );
        }
        assert_eq!(writer.0.borrow().close_calls, 0);
        reclaimed(messages);
    });
}

#[test]
fn network_shutdown_wakes_parked_receive_and_releases_partial_frame() {
    with_peer(|driver, messages| {
        let network = Network::new();
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let input = Rc::new(RefCell::new(Pipe {
            bytes: [0; 3].into(),
            ..Default::default()
        }));
        let (mut reader, mut writer) = (Reader(input.clone()), Writer(Rc::default()));
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let mut disconnected = network.get_disconnector();
        {
            let mut run = std::pin::pin!(registered.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                std::future::pending(),
                std::future::pending()
            ));
            for _ in 0..20 {
                assert!(run.as_mut().poll(&mut cx).is_pending());
            }
            assert!(input.borrow().bytes.is_empty());
            let reads = input.borrow().read_calls;
            let before = wakes.0.load(Ordering::SeqCst);
            assert!(poll(&mut disconnected).is_pending());
            assert!(
                wakes.0.load(Ordering::SeqCst) > before,
                "network stop must wake parked IO"
            );
            let mut finished = false;
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    result.unwrap();
                    finished = true;
                    break;
                }
            }
            assert!(finished);
            assert_eq!(
                input.borrow().read_calls,
                reads,
                "stop precedes further input"
            );
        }
        assert!(matches!(poll(&mut disconnected), Poll::Ready(Ok(()))));
        reclaimed(messages);
    });
}

#[test]
fn canceling_registered_drain_retires_owner_without_claiming_success() {
    with_peer(|driver, messages| {
        let network = Network::new();
        let registered = coordinator(driver).register(&network, 1).unwrap();
        let (mut reader, mut writer) = (
            Reader(Rc::default()),
            Writer(Rc::new(RefCell::new(Pipe {
                block_close: true,
                ..Default::default()
            }))),
        );
        let output = writer.0.clone();
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        let mut disconnected = network.get_disconnector();
        let mut run = Box::pin(registered.run_stream(
            messages,
            &mut queue,
            &mut reader,
            &mut writer,
            &mut scratch,
            |_| (),
            &mut policy,
            async { Ok(()) },
            std::future::pending(),
        ));
        for _ in 0..2000 {
            assert!(poll(&mut run).is_pending());
            if output.borrow().close_calls > 0 {
                break;
            }
        }
        assert!(network.connection(1).is_none());
        assert!(output.borrow().close_calls > 0 && !output.borrow().closed);
        assert!(poll(&mut disconnected).is_pending());
        drop(run);
        assert!(
            matches!(poll(&mut disconnected), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected)
        );
        // Early cancellation requires ending this pool scope, unlike completed cleanup.
    });
}

#[test]
fn registered_network_bootstrap_runs_fragmented_rpc_and_disconnects_both_peers() {
    with_peer(|a, am| {
        with_peer(|b, bm| {
            let network = Network::new();
            let (a, b) = (
                coordinator(a).register(&network, 1).unwrap(),
                coordinator(b).register(&network, 2).unwrap(),
            );
            let client: capability::Client = network.connection(1).unwrap().bootstrap();
            let mut request: capability::Request<any_pointer::Owned, any_pointer::Owned> =
                client.new_call(7, 0, None);
            let payload = vec![29; if cfg!(miri) { 257 } else { 65536 }];
            request
                .get()
                .set_as::<capnp::data::Owned>(payload.as_slice())
                .unwrap();
            let mut response = request.send().promise;
            let (ab, ba) = (
                Rc::new(RefCell::new(Pipe::default())),
                Rc::new(RefCell::new(Pipe::default())),
            );
            let (mut ar, mut aw, mut br, mut bw) = (
                Reader(ba.clone()),
                Writer(ab.clone()),
                Reader(ab),
                Writer(ba),
            );
            let seen = Rc::new(Cell::new(0));
            let hook: Rc<dyn ClientHook> = Rc::new(crate::local::Client::new(Echo(seen.clone())));
            let mut ap = Policy::default();
            let mut bp = TwoParty::new(
                || Ok(capability::Client::new(hook.add_ref())),
                NonZeroUsize::new(4).unwrap(),
            );
            let (mut aq, mut bq) = (
                messages::Queue::new_growing(0, 1).unwrap(),
                messages::Queue::new_growing(0, 1).unwrap(),
            );
            let (mut ascratch, mut bscratch) = ([0; 32], [0; 32]);
            let mut disconnected = network.get_disconnector();
            {
                let mut af = std::pin::pin!(a.run_stream(
                    am,
                    &mut aq,
                    &mut ar,
                    &mut aw,
                    &mut ascratch,
                    |_| (),
                    &mut ap,
                    std::future::pending(),
                    std::future::pending()
                ));
                let mut bf = std::pin::pin!(b.run_stream(
                    bm,
                    &mut bq,
                    &mut br,
                    &mut bw,
                    &mut bscratch,
                    |_| (),
                    &mut bp,
                    std::future::pending(),
                    std::future::pending()
                ));
                let mut received = false;
                for _ in 0..20000 {
                    assert!(poll(&mut af).is_pending());
                    assert!(poll(&mut bf).is_pending());
                    if let Poll::Ready(result) = poll(&mut response) {
                        assert_eq!(
                            result
                                .unwrap()
                                .get()
                                .unwrap()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            payload
                        );
                        received = true;
                        break;
                    }
                }
                assert!(received && seen.get() == 1);
                drop(client);
                assert!(poll(&mut disconnected).is_pending());
                let (mut adone, mut bdone) = (false, false);
                for _ in 0..2000 {
                    if !adone {
                        if let Poll::Ready(result) = poll(&mut af) {
                            result.unwrap();
                            adone = true;
                        }
                    }
                    if !bdone {
                        if let Poll::Ready(result) = poll(&mut bf) {
                            result.unwrap();
                            bdone = true;
                        }
                    }
                    if adone && bdone {
                        break;
                    }
                }
                assert!(adone && bdone);
            }
            assert!(matches!(poll(&mut disconnected), Poll::Ready(Ok(()))));
            assert!(aw.0.borrow().closed && bw.0.borrow().closed);
            reclaimed(am);
            reclaimed(bm);
        })
    });
}
