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
