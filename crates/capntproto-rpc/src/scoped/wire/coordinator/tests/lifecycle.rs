use super::*;
use crate::rpc_capnp;

pub(super) fn queued<'p>(
    driver: &mut TestPeer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> Vec<u8> {
    driver
        .send(
            messages,
            queue,
            512,
            Default::default(),
            capntproto_rpc_wire::calls::bootstrap,
        )
        .unwrap();
    queue.window(messages).unwrap().unwrap().bytes().to_vec()
}

#[test]
fn orderly_stop_and_eof_drain_flush_and_close_before_completion() {
    for eof in [false, true] {
        with_peer(|mut driver, messages| {
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let expected = queued(&mut driver, messages, &mut queue);
            let coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let input = Rc::new(RefCell::new(Pipe {
                eof,
                ..Default::default()
            }));
            let output = Rc::new(RefCell::new(Pipe {
                block_close: true,
                ..Default::default()
            }));
            let handle = coordinator.handle();
            let command = (!eof).then(|| handle.bootstrap::<capability::Client>());
            let mut reader = Reader(input.clone());
            let mut writer = Writer(output.clone());
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            let wakes = Arc::new(WakeCount::default());
            let waker = Waker::from(wakes.clone());
            let mut cx = Context::from_waker(&waker);
            {
                let shutdown = async {
                    if eof {
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                };
                let mut run = std::pin::pin!(coordinator.run_stream(
                    messages,
                    &mut queue,
                    &mut reader,
                    &mut writer,
                    &mut scratch,
                    |_| (),
                    &mut policy,
                    shutdown,
                    std::future::pending(),
                ));
                let mut reached_close = false;
                for _ in 0..2000 {
                    assert!(run.as_mut().poll(&mut cx).is_pending());
                    if output.borrow().close_calls > 0 {
                        reached_close = true;
                        break;
                    }
                }
                assert!(reached_close);
                let bytes: Vec<_> = output.borrow().bytes.iter().copied().collect();
                assert_eq!(&bytes[..expected.len()], expected);
                let mut terminal = &bytes[expected.len()..];
                if eof {
                    let message = capnp::serialize::read_message_from_flat_slice_no_alloc(
                        &mut terminal,
                        Default::default(),
                    )
                    .unwrap();
                    let rpc_capnp::message::Abort(error) = message
                        .get_root::<rpc_capnp::message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!("one terminal Abort after accepted output")
                    };
                    let error = error.unwrap();
                    assert_eq!(
                        error.get_type().unwrap(),
                        rpc_capnp::exception::Type::Disconnected
                    );
                    assert_eq!(
                        error.get_reason().unwrap().to_str().unwrap(),
                        "client requested disconnect"
                    );
                    assert!(terminal.is_empty());
                } else {
                    // The existing frame fills this one-entry FIFO when stop
                    // wins; best-effort Abort must not replace accepted output.
                    assert!(terminal.is_empty());
                }
                assert!(output.borrow().flush_calls > 0);
                assert!(!output.borrow().closed);
                if let Some(command) = command {
                    assert!(
                        matches!(Pin::new(&mut command.when_resolved()).poll(&mut cx),
                        Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected)
                    );
                }
                let late: capability::Client = handle.bootstrap();
                assert!(matches!(Pin::new(&mut late.when_resolved()).poll(&mut cx),
                    Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Disconnected));
                let counts = {
                    let output = output.borrow();
                    (
                        output.write_calls,
                        output.flush_calls,
                        input.borrow().read_calls,
                    )
                };
                let before = wakes.0.load(Ordering::SeqCst);
                for _ in 0..4 {
                    assert!(run.as_mut().poll(&mut cx).is_pending());
                }
                assert_eq!(
                    wakes.0.load(Ordering::SeqCst),
                    before,
                    "blocked close must sleep"
                );
                output.borrow_mut().block_close = false;
                output.borrow_mut().write_wake.take().unwrap().wake();
                let mut done = false;
                for _ in 0..2000 {
                    if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                        result.unwrap();
                        done = true;
                        break;
                    }
                }
                assert!(done);
                let output = output.borrow();
                assert_eq!(
                    (
                        output.write_calls,
                        output.flush_calls,
                        input.borrow().read_calls
                    ),
                    counts
                );
                assert!(output.closed);
            }
            assert!(policy.stopped && queue.is_empty());
            reclaimed(messages);
        });
    }
}

#[test]
fn shutdown_abort_cancels_blocked_write_flush_and_close_and_preserves_its_reason() {
    for stage in 0..3 {
        with_peer(|mut driver, messages| {
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            queued(&mut driver, messages, &mut queue);
            let coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let input = Rc::new(RefCell::new(Pipe {
                bytes: [0; 3].into(),
                ..Default::default()
            }));
            let output = Rc::new(RefCell::new(Pipe {
                blocked: stage == 0,
                block_flush: stage == 1,
                block_close: stage == 2,
                ..Default::default()
            }));
            let mut reader = Reader(input.clone());
            let mut writer = Writer(output.clone());
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            let abort = Signal::default();
            let shutdown = Signal::default();
            let mut cx = Context::from_waker(Waker::noop());
            {
                let mut run = std::pin::pin!(coordinator.run_stream(
                    messages,
                    &mut queue,
                    &mut reader,
                    &mut writer,
                    &mut scratch,
                    |_| (),
                    &mut policy,
                    shutdown.wait(),
                    async {
                        abort.wait().await.unwrap();
                        Error::overloaded("caller drain deadline".into())
                    },
                ));
                for _ in 0..20 {
                    assert!(run.as_mut().poll(&mut cx).is_pending());
                }
                assert!(
                    input.borrow().bytes.is_empty(),
                    "partial read must be in flight"
                );
                shutdown.stop();
                // Protocol retirement is budgeted before physical output starts.
                for _ in 0..2000 {
                    assert!(run.as_mut().poll(&mut cx).is_pending());
                    let output = output.borrow();
                    let reached = match stage {
                        0 => output.write_calls > 0,
                        1 => output.flush_calls > 0,
                        _ => output.close_calls > 0,
                    };
                    if reached {
                        break;
                    }
                }
                let counts = {
                    let output = output.borrow();
                    assert!(output.write_calls > 0);
                    if stage >= 1 {
                        assert!(output.flush_calls > 0);
                    }
                    if stage == 2 {
                        assert!(output.close_calls > 0);
                    }
                    (
                        output.write_calls,
                        output.flush_calls,
                        output.close_calls,
                        input.borrow().read_calls,
                    )
                };
                abort.stop();
                let mut done = false;
                for _ in 0..2000 {
                    if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                        let error = result.unwrap_err();
                        assert!(error.cleanup.is_none());
                        let Some(Cause::Protocol(error)) = error.cause else {
                            panic!("lost abort reason")
                        };
                        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
                        assert_eq!(error.extra, "caller drain deadline");
                        done = true;
                        break;
                    }
                }
                assert!(done);
                let output = output.borrow();
                assert_eq!(
                    (
                        output.write_calls,
                        output.flush_calls,
                        output.close_calls,
                        input.borrow().read_calls
                    ),
                    counts,
                    "abort must win before any further IO"
                );
                assert!(!output.closed);
            }
            assert!(policy.stopped && queue.is_empty());
            reclaimed(messages);
        });
    }
}

#[test]
fn protocol_failure_and_physical_close_failure_are_preserved_independently() {
    with_peer(|driver, messages| {
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let mut reader = Reader(Rc::default());
        let mut writer = Writer(Rc::new(RefCell::new(Pipe {
            fail_close: true,
            ..Default::default()
        })));
        let mut policy = Policy::default();
        let mut scratch = [0; 32];
        {
            let mut run = std::pin::pin!(coordinator.run_stream(
                messages,
                &mut queue,
                &mut reader,
                &mut writer,
                &mut scratch,
                |_| (),
                &mut policy,
                async { Err(Error::overloaded("original failure".into())) },
                std::future::pending(),
            ));
            let mut completed = false;
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..2000 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    let error = result.unwrap_err();
                    assert!(
                        matches!(error.cause, Some(Cause::Protocol(error)) if error.extra == "original failure")
                    );
                    assert_eq!(error.close, Some("close failed"));
                    assert!(error.cleanup.is_none());
                    completed = true;
                    break;
                }
            }
            assert!(completed);
        }
        assert!(queue.is_empty() && policy.stopped);
        reclaimed(messages);
    });
}

#[test]
fn flush_and_close_failures_are_reported_after_protocol_storage_is_reclaimed() {
    for close in [false, true] {
        with_peer(|mut driver, messages| {
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            queued(&mut driver, messages, &mut queue);
            let coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let mut reader = Reader(Rc::new(RefCell::new(Pipe::default())));
            let output = Rc::new(RefCell::new(Pipe {
                fail_flush: !close,
                fail_close: close,
                ..Default::default()
            }));
            let mut writer = Writer(output.clone());
            let mut policy = Policy::default();
            let mut scratch = [0; 32];
            let mut cx = Context::from_waker(Waker::noop());
            {
                let mut run = std::pin::pin!(coordinator.run_stream(
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
                let mut done = false;
                for _ in 0..2000 {
                    if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                        let error = result.unwrap_err();
                        assert!(error.cleanup.is_none());
                        if close {
                            assert!(error.cause.is_none());
                            assert_eq!(error.close, Some("close failed"));
                        } else {
                            assert!(matches!(
                                error.cause,
                                Some(Cause::Output(io::SendError::Io("flush failed")))
                            ));
                        }
                        done = true;
                        break;
                    }
                }
                assert!(done);
            }
            assert!(!output.borrow().closed);
            assert!(policy.stopped && queue.is_empty());
            reclaimed(messages);
        });
    }
}
