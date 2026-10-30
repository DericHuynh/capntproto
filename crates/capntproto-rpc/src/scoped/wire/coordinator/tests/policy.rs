use super::*;

#[test]
fn idle_is_revoked_before_prequeued_and_same_turn_writes() {
    struct CheckedWriter {
        idle: Rc<Cell<bool>>,
        bytes: usize,
    }
    impl io::Flush for CheckedWriter {
        type Error = ();
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<()> for CheckedWriter {
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
            _: Option<&()>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            assert!(
                !self.idle.get(),
                "write must follow the active notification"
            );
            self.bytes += bytes.len();
            Poll::Ready(Ok(bytes.len()))
        }
    }
    struct ControlWriter {
        idle: Rc<Cell<bool>>,
        notifications: Vec<bool>,
        publish: bool,
        wake: Option<Waker>,
    }
    fn publish<'p>(
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
    ) {
        let writer = messages.reserve_exact(8).unwrap();
        let lease = messages.seal(writer, 8).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue.reserve().unwrap().accept(messages, offer);
        messages.release(lease);
    }
    impl<'p, 'id, 's> Control<'p, 'id, 's, (), u8, 8, 8> for ControlWriter {
        fn preserves_idle(&self, _: Activity) -> bool {
            false
        }
        fn set_idle(&mut self, idle: bool) {
            assert_ne!(self.idle.replace(idle), idle, "duplicate notification");
            self.notifications.push(idle);
        }
        fn received(
            &mut self,
            _: connection::Received<'id, server::Producer>,
            _: &mut ControlContext<'_, 'p, 'id, 's, (), u8, 8, 8>,
            _: &mut messages::Messages<'p, '_, '_>,
            _: &mut messages::Queue<'p, '_>,
        ) -> capnp::Result<()> {
            unreachable!()
        }
        fn poll(
            &mut self,
            _: &mut ControlContext<'_, 'p, 'id, 's, (), u8, 8, 8>,
            messages: &mut messages::Messages<'p, '_, '_>,
            queue: &mut messages::Queue<'p, '_>,
            _: &mut [u32],
            _: &NoResources,
            cx: &mut Context<'_>,
        ) -> Poll<capnp::Result<bool>> {
            self.wake = Some(cx.waker().clone());
            if std::mem::take(&mut self.publish) {
                // Hosted publication can enter the physical FIFO immediately,
                // before the later output poll reaches CheckedWriter.
                assert!(
                    !self.idle.get(),
                    "control publication must follow active notification"
                );
                publish(messages, queue);
                Poll::Ready(Ok(true))
            } else {
                Poll::Pending
            }
        }
        fn shutdown(
            &mut self,
            _: &mut ControlContext<'_, 'p, 'id, 's, (), u8, 8, 8>,
            _: &mut messages::Messages<'p, '_, '_>,
        ) {
        }
    }
    fn poll<'p, 'id, 's>(
        coordinator: &mut TestCoordinator<'p, 'id, 's>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
        writer: &mut CheckedWriter,
        control: &mut ControlWriter,
    ) {
        coordinator
            .poll_io_work::<(), _, _>(
                messages,
                queue,
                writer,
                &mut [0; 32],
                &NoResources,
                control,
                &mut Context::from_waker(Waker::noop()),
            )
            .unwrap();
    }
    for prequeued in [false, true] {
        with_peer(|driver, messages| {
            let mut coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let idle = Rc::new(Cell::new(false));
            let mut writer = CheckedWriter {
                idle: idle.clone(),
                bytes: 0,
            };
            let mut control = ControlWriter {
                idle: idle.clone(),
                notifications: Vec::new(),
                publish: false,
                wake: None,
            };
            for _ in 0..512 {
                poll(
                    &mut coordinator,
                    messages,
                    &mut queue,
                    &mut writer,
                    &mut control,
                );
            }
            assert!(idle.get());
            assert_eq!(control.notifications, [true]);
            if prequeued {
                publish(messages, &mut queue);
            } else {
                control.publish = true;
                control.wake.as_ref().unwrap().wake_by_ref();
            }
            poll(
                &mut coordinator,
                messages,
                &mut queue,
                &mut writer,
                &mut control,
            );
            assert_eq!(writer.bytes, 8);
            assert!(control.notifications.starts_with(&[true, false]));
            for _ in 0..512 {
                poll(
                    &mut coordinator,
                    messages,
                    &mut queue,
                    &mut writer,
                    &mut control,
                );
            }
            assert_eq!(control.notifications, [true, false, true]);
            drop(coordinator);
            reclaimed(messages);
        });
    }
}

#[test]
fn idle_requires_flushed_output_even_without_protocol_references() {
    with_peer(|driver, messages| {
        assert!(driver.is_idle());
        let mut queue = messages::Queue::new_growing(0, 1).unwrap();
        let buffer = messages.reserve(8).unwrap();
        messages.write(&buffer)[..8].fill(0);
        let lease = messages.seal(buffer, 8).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue.reserve().unwrap().accept(messages, offer);
        messages.release(lease);
        let coordinator = Coordinator::new(
            driver,
            ClientHooks::new(8, 8),
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let input = Rc::new(RefCell::new(Pipe::default()));
        let output = Rc::new(RefCell::new(Pipe {
            block_flush: true,
            ..Default::default()
        }));
        let mut reader = Reader(input);
        let mut writer = Writer(output.clone());
        let idle = Rc::new(RefCell::new(Vec::new()));
        let observer = idle.clone();
        let mut control = TwoParty::new(
            || Err(Error::unimplemented("no bootstrap".into())),
            NonZeroUsize::new(1).unwrap(),
        )
        .with_idle_notifier(move |state| observer.borrow_mut().push(state));
        let stop = Signal::default();
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
                &mut control,
                stop.wait(),
                std::future::pending(),
            ));
            for _ in 0..64 {
                assert!(run.as_mut().poll(&mut cx).is_pending());
            }
            assert!(!output.borrow().buffered.is_empty());
            assert!(output.borrow().flush_calls > 0);
            assert!(idle.borrow().is_empty(), "buffered output is still busy");
            output.borrow_mut().block_flush = false;
            output.borrow_mut().write_wake.take().unwrap().wake();
            for _ in 0..64 {
                assert!(run.as_mut().poll(&mut cx).is_pending());
            }
            assert!(output.borrow().buffered.is_empty());
            assert_eq!(&*idle.borrow(), &[true]);
            stop.stop();
            let mut done = false;
            for _ in 0..64 {
                if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                    result.unwrap();
                    done = true;
                    break;
                }
            }
            assert!(done);
        }
        reclaimed(messages);
    });
}

#[test]
fn output_completion_preserves_only_an_explicitly_idle_control_policy() {
    for preserve in [false, true] {
        with_peer(|driver, messages| {
            let mut coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let output = Rc::new(RefCell::new(Pipe::default()));
            let mut writer = Writer(output.clone());
            let mut control = Policy {
                preserve_plain: preserve,
                ..Default::default()
            };
            let count = Arc::new(WakeCount(AtomicUsize::new(0)));
            let wake = Waker::from(count.clone());
            let mut cx = Context::from_waker(&wake);
            for _ in 0..32 {
                coordinator
                    .poll_io_work::<(), _, _>(
                        messages,
                        &mut queue,
                        &mut transport::Writer(&mut writer),
                        &mut [0; 32],
                        &NoResources,
                        &mut control,
                        &mut cx,
                    )
                    .unwrap();
                if coordinator.schedule.active == WorkSet::NONE && coordinator.control_idle {
                    break;
                }
            }
            assert_eq!(coordinator.schedule.active, WorkSet::NONE);
            assert!(coordinator.control_idle);
            let polls = control.polls;
            count.0.store(0, Ordering::SeqCst);
            let buffer = messages.reserve_exact(8).unwrap();
            let frame = messages.seal(buffer, 8).unwrap();
            let offer = messages.offer(&frame).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(frame);
            coordinator
                .poll_io_work::<(), _, _>(
                    messages,
                    &mut queue,
                    &mut transport::Writer(&mut writer),
                    &mut [0; 32],
                    &NoResources,
                    &mut control,
                    &mut cx,
                )
                .unwrap();
            assert!(queue.is_empty() && coordinator.output.is_clean());
            assert_eq!(output.borrow().bytes.len(), 8);
            assert_eq!(coordinator.schedule.active, WorkSet::NONE);
            assert!(coordinator.control_idle);
            assert_eq!(control.polls, polls + usize::from(!preserve));
            assert_eq!(
                count.0.load(Ordering::SeqCst),
                0,
                "output-only progress added an executor turn"
            );
            drop(coordinator);
            reclaimed(messages);
        });
    }
}

#[test]
fn continuously_publishing_control_charges_each_output_continuation() {
    struct Sink(usize);
    impl io::Flush for Sink {
        type Error = ();
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            Poll::Ready(Ok(()))
        }
    }
    impl io::Write for Sink {
        fn poll_write(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<Result<usize, io::IoError<()>>> {
            self.0 += bytes.len();
            Poll::Ready(Ok(bytes.len()))
        }
    }
    for budget in [1, 7, 64] {
        with_peer(|driver, messages| {
            let mut opts = options();
            opts.budget = NonZeroUsize::new(budget).unwrap();
            let mut coordinator = Coordinator::new(
                driver,
                ClientHooks::new(8, 8),
                NonZeroUsize::new(4).unwrap(),
                None,
                opts,
            )
            .unwrap();
            coordinator.schedule.active = WorkSet::NONE;
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let mut writer = Sink(0);
            let mut control = Policy {
                publish_repeatedly: true,
                ..Default::default()
            };
            assert!(!coordinator
                .poll_io_work::<(), _, _>(
                    messages,
                    &mut queue,
                    &mut transport::Writer(&mut writer),
                    &mut [0; 32],
                    &NoResources,
                    &mut control,
                    &mut Context::from_waker(Waker::noop())
                )
                .unwrap());
            assert_eq!(
                control.polls, budget,
                "output production bypassed the cooperative budget"
            );
            assert_eq!(writer.0, budget * 8);
            assert!(queue.is_empty() && coordinator.output.is_clean());
            futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                messages,
                &mut queue,
                &mut control,
                Ok(()),
            ))
            .unwrap();
            reclaimed(messages);
        });
    }
}
