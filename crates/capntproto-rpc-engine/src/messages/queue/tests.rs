use super::*;
use crate::io::{self, IoError};
use core::{
    num::NonZeroUsize,
    task::{Context, Poll, Waker},
};

#[test]
fn nonempty_queue_storage_rejection_preserves_its_lease_and_resources() {
    struct Resource<'a>(&'a core::cell::Cell<usize>);
    impl Drop for Resource<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let dropped = core::cell::Cell::new(0);
    let mut bytes = [0; 8];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [], |messages| {
        let lease = messages.copy_from(b"retained").unwrap();
        let mut entries = [Some(QueueEntry {
            message: QueuedMessage::Owned { lease, position: 0 },
            resources: Resource(&dropped),
        })];
        assert!(matches!(
            Queue::with_resources(&mut entries),
            Err(Error::NonemptyStorage)
        ));
        assert_eq!(dropped.get(), 0);
        let entry = entries[0].take().unwrap();
        let QueuedMessage::Owned { lease, .. } = entry.message else {
            panic!("original owner")
        };
        assert_eq!(&*messages.read(&lease), b"retained");
        messages.release(lease);
        drop(entry.resources);
        assert_eq!(dropped.get(), 1);
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        let lease = messages.copy_from(b"reused").unwrap();
        queue
            .reserve_with(Resource(&dropped))
            .unwrap()
            .accept_owned(messages, lease);
        assert!(queue.window(messages).unwrap().unwrap().advance(6).unwrap());
        assert_eq!(dropped.get(), 2);
    })
    .unwrap();
}

#[test]
fn owned_fifo_needs_no_send_slots_and_preserves_resources_through_partial_writes() {
    struct Resource<'a>(&'a core::cell::Cell<usize>);
    impl Drop for Resource<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let dropped = core::cell::Cell::new(0);
    let mut first = [0; 3];
    let mut second = [0; 3];
    let mut buffers = [Buffer::new(&mut first), Buffer::new(&mut second)];
    let usage = allocation_counter::measure(|| {
        with_messages(&mut buffers, &mut [], |messages| {
            let mut entries = [const { None }; 2];
            let mut queue = Queue::with_resources(&mut entries).unwrap();
            for bytes in [b"abc", b"def"] {
                let lease = messages.copy_from(bytes).unwrap();
                queue
                    .reserve_with(Resource(&dropped))
                    .unwrap()
                    .accept_owned(messages, lease);
            }
            assert!(matches!(
                queue.reserve_with(Resource(&dropped)),
                Err(Error::NoSendSlot)
            ));
            assert_eq!(dropped.get(), 1); // rejected reservation releases only itself.
            let window = queue.window(messages).unwrap().unwrap();
            assert!(window.resources().is_some());
            assert_eq!(window.advance(0), Err(Error::WriteZero));
            assert!(queue
                .window(messages)
                .unwrap()
                .unwrap()
                .resources()
                .is_some());
            assert!(!queue.window(messages).unwrap().unwrap().advance(1).unwrap());
            let window = queue.window(messages).unwrap().unwrap();
            assert_eq!(window.bytes(), b"bc");
            assert!(window.resources().is_none());
            assert_eq!(window.advance(3), Err(Error::InvalidWriteCount));
            assert_eq!(queue.window(messages).unwrap().unwrap().bytes(), b"bc");
            assert!(queue.window(messages).unwrap().unwrap().advance(2).unwrap());
            assert_eq!(dropped.get(), 2);
            let lease = messages.copy_from(b"ghi").unwrap();
            queue
                .reserve_with(Resource(&dropped))
                .unwrap()
                .accept_owned(messages, lease);
            assert_eq!(queue.window(messages).unwrap().unwrap().bytes(), b"def");
            queue.fail();
            assert!(queue.discard_one(messages).unwrap());
            assert!(queue.discard_one(messages).unwrap());
            assert!(!queue.discard_one(messages).unwrap());
            assert_eq!(dropped.get(), 4);
            let a = messages.reserve(3).unwrap();
            let b = messages.reserve(3).unwrap();
            messages.discard(a);
            messages.discard(b);
        })
        .unwrap();
    });
    assert_eq!(usage.count_total, 0);
}

#[test]
fn tracked_and_owned_entries_share_one_ordered_fifo() {
    for owned_first in [false, true] {
        let mut a = [0; 3];
        let mut b = [0; 3];
        let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
        with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
            let mut entries = [const { None }; 2];
            let mut queue = Queue::new(&mut entries).unwrap();
            for (index, text) in [b"abc", b"def"].into_iter().enumerate() {
                let lease = messages.copy_from(text).unwrap();
                if (index == 0) == owned_first {
                    queue.reserve().unwrap().accept_owned(messages, lease);
                } else {
                    let offer = messages.offer_owned(lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                }
            }
            for expected in [b"abc", b"def"] {
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.bytes(), expected);
                assert!(!window.advance(1).unwrap());
                assert_eq!(
                    queue.window(messages).unwrap().unwrap().bytes(),
                    &expected[1..]
                );
                assert!(queue.window(messages).unwrap().unwrap().advance(2).unwrap());
            }
            assert!(queue.is_empty());
        })
        .unwrap();
    }
}

#[cfg(feature = "alloc")]
#[test]
fn owned_fifo_shared_output_pins_pool_after_completion_and_failure() {
    for failed in [false, true] {
        let mut pool = GrowingBuffers::new(1, 16, 23);
        let shared = with_growing_messages(
            &mut pool,
            crate::connection::Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let mut entries = [None];
                let mut queue = Queue::new(&mut entries).unwrap();
                let lease = messages.copy_from(b"retained").unwrap();
                queue.reserve().unwrap().accept_owned(messages, lease);
                let window = queue.window(messages).unwrap().unwrap();
                let shared = window.share().unwrap();
                if failed {
                    drop(window);
                    queue.fail();
                    assert!(queue.discard_one(messages).unwrap());
                } else {
                    assert!(window.advance(8).unwrap());
                }
                assert_eq!(messages.reserve(8).unwrap_err(), Error::NoBuffer);
                shared
            },
        )
        .unwrap();
        drop(pool);
        assert_eq!(&*shared, b"retained");
    }
}

#[test]
fn capacity_probe_neither_builds_resources_nor_reserves_a_publication() {
    struct Resources;
    impl Default for Resources {
        fn default() -> Self {
            panic!("capacity probing must not build resources")
        }
    }
    let mut bytes = [0; 8];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let mut entries = [None];
        let mut queue = Queue::<Resources>::with_resources(&mut entries).unwrap();
        queue.ensure_capacity().unwrap();
        queue.ensure_capacity().unwrap();
        assert!(queue.is_empty());
        let writer = messages.reserve(8).unwrap();
        let lease = messages.seal(writer, 8).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue
            .reserve_with(Resources)
            .unwrap()
            .accept(messages, offer);
        messages.release(lease);
        assert_eq!(queue.ensure_capacity(), Err(Error::NoSendSlot));
        assert!(queue.window(messages).unwrap().unwrap().advance(8).unwrap());
        queue.ensure_capacity().unwrap();
        queue.fail();
        assert_eq!(queue.ensure_capacity(), Err(Error::QueueFailed));
    })
    .unwrap();
}

#[test]
fn fifo_reuses_ring_slots_without_overtaking_partial_frames_or_losing_pending_progress() {
    let mut first = [0; 3];
    let mut second = [0; 3];
    let mut buffers = [Buffer::new(&mut first), Buffer::new(&mut second)];
    let mut sends = [SendSlot::EMPTY, SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let mut entries = [const { None }; 2];
        let mut queue = Queue::new(&mut entries).unwrap();
        for text in [b"abc", b"def"] {
            let writer = messages.reserve(3).unwrap();
            messages.write(&writer).copy_from_slice(text);
            let lease = messages.seal(writer, 3).unwrap();
            let offer = messages.offer(&lease).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(lease);
        }
        assert!(matches!(queue.reserve(), Err(Error::NoSendSlot)));
        let window = queue.window(messages).unwrap().unwrap();
        assert_eq!(window.bytes(), b"abc");
        assert_eq!(window.advance(1), Ok(false));
        assert_eq!(queue.len(), 2);
        {
            assert_eq!(queue.window(messages).unwrap().unwrap().bytes(), b"bc");
        }
        assert_eq!(
            queue.window(messages).unwrap().unwrap().advance(3),
            Err(Error::InvalidWriteCount)
        );
        assert_eq!(queue.window(messages).unwrap().unwrap().bytes(), b"bc");
        queue.window(messages).unwrap().unwrap().advance(2).unwrap();
        let writer = messages.reserve(3).unwrap();
        messages.write(&writer).copy_from_slice(b"ghi");
        let lease = messages.seal(writer, 3).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue.reserve().unwrap().accept(messages, offer);
        messages.release(lease);
        for text in [b"def", b"ghi"] {
            let window = queue.window(messages).unwrap().unwrap();
            assert_eq!(window.bytes(), text);
            assert!(window.advance(3).unwrap());
        }
        assert!(queue.window(messages).unwrap().is_none());
    })
    .unwrap();
}

struct Fault {
    calls: usize,
    fail_at: usize,
    zero: bool,
}
impl crate::io::Flush for Fault {
    type Error = u8;
    fn poll_flush(
        &mut self,
        _: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Result<(), crate::io::IoError<Self::Error>>> {
        core::task::Poll::Ready(Ok(()))
    }
}
impl io::Write for Fault {
    fn poll_write(&mut self, _: &mut Context<'_>, _: &[u8]) -> Poll<Result<usize, IoError<u8>>> {
        self.calls += 1;
        if self.calls == self.fail_at {
            if self.zero {
                Poll::Ready(Ok(0))
            } else {
                Poll::Ready(Err(IoError::Failed(9)))
            }
        } else {
            Poll::Ready(Ok(1))
        }
    }
}
#[test]
fn every_mid_frame_io_failure_is_terminal_and_never_exposes_the_next_frame() {
    for owned in [false, true] {
        for fail_at in 1..=3 {
            for zero in [false, true] {
                let mut a = [0; 3];
                let mut b = [0; 3];
                let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
                let mut sends = [SendSlot::EMPTY, SendSlot::EMPTY];
                with_messages(&mut buffers, &mut sends, |messages| {
                    let mut entries = [const { None }; 2];
                    let mut queue = Queue::new(&mut entries).unwrap();
                    for _ in 0..2 {
                        let w = messages.reserve(3).unwrap();
                        let l = messages.seal(w, 3).unwrap();
                        if owned {
                            queue.reserve().unwrap().accept_owned(messages, l);
                        } else {
                            let o = messages.offer(&l).unwrap();
                            queue.reserve().unwrap().accept(messages, o);
                            messages.release(l);
                        }
                    }
                    let mut writer = Fault {
                        calls: 0,
                        fail_at,
                        zero,
                    };
                    let mut cx = Context::from_waker(Waker::noop());
                    let expected = if zero {
                        io::SendError::State(Error::WriteZero)
                    } else {
                        io::SendError::Io(9)
                    };
                    assert_eq!(
                        io::poll_queue(
                            &mut writer,
                            &mut queue,
                            messages,
                            &mut cx,
                            NonZeroUsize::new(8).unwrap()
                        ),
                        Poll::Ready(Err(expected))
                    );
                    assert!(queue.is_failed());
                    assert_eq!(queue.len(), 2);
                    assert!(matches!(queue.reserve(), Err(Error::QueueFailed)));
                    assert!(matches!(
                        io::poll_queue(
                            &mut writer,
                            &mut queue,
                            messages,
                            &mut cx,
                            NonZeroUsize::new(8).unwrap()
                        ),
                        Poll::Ready(Err(io::SendError::State(Error::QueueFailed)))
                    ));
                    assert_eq!(writer.calls, fail_at);
                })
                .unwrap();
            }
        }
    }
}

#[test]
fn externally_failed_send_makes_fifo_terminal_before_any_further_io() {
    let mut bytes = [0; 3];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let mut entries = [None];
        let mut queue = Queue::new(&mut entries).unwrap();
        let w = messages.reserve(3).unwrap();
        let l = messages.seal(w, 3).unwrap();
        let offer = messages.offer(&l).unwrap();
        let key = queue.reserve().unwrap().accept(messages, offer);
        messages.release(l);
        messages.fail_send(key).unwrap();
        let mut writer = Fault {
            calls: 0,
            fail_at: 1,
            zero: false,
        };
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            io::poll_queue(
                &mut writer,
                &mut queue,
                messages,
                &mut cx,
                NonZeroUsize::new(1).unwrap()
            ),
            Poll::Ready(Err(io::SendError::State(Error::StaleSend)))
        ));
        assert_eq!(writer.calls, 0);
        assert!(queue.is_failed());
        assert!(matches!(queue.reserve(), Err(Error::QueueFailed)));
    })
    .unwrap();
}

struct Resource<'a> {
    drops: &'a core::cell::Cell<u32>,
    bit: u32,
}
impl Drop for Resource<'_> {
    fn drop(&mut self) {
        assert_eq!(self.drops.get() & self.bit, 0, "resource dropped twice");
        self.drops.set(self.drops.get() | self.bit);
    }
}

#[test]
fn resources_follow_exact_fifo_frame_across_pending_interrupted_and_partial_writes() {
    use core::cell::Cell;
    struct Writer {
        attempts: usize,
        received: u32,
    }
    impl crate::io::Flush for Writer {
        type Error = ();
        fn poll_flush(
            &mut self,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), crate::io::IoError<Self::Error>>> {
            core::task::Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<Resource<'_>> for Writer {
        fn poll_write_resources(
            &mut self,
            cx: &mut Context<'_>,
            bytes: &[u8],
            resource: Option<&Resource<'_>>,
        ) -> Poll<Result<usize, IoError<()>>> {
            self.attempts += 1;
            match self.attempts {
                1 | 4 => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                2 => return Poll::Ready(Err(IoError::Interrupted)),
                _ => (),
            }
            if let Some(resource) = resource {
                assert_eq!(self.received & resource.bit, 0, "duplicate transmission");
                assert_eq!(bytes[0], if resource.bit == 1 { b'a' } else { b'd' });
                self.received |= resource.bit;
            }
            Poll::Ready(Ok(1))
        }
    }
    let usage = allocation_counter::measure(|| {
        let drops = Cell::new(0);
        let mut a = [0; 3];
        let mut b = [0; 3];
        let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
        with_messages(
            &mut buffers,
            &mut [const { SendSlot::EMPTY }; 2],
            |messages| {
                let mut entries = [const { None }; 2];
                let mut queue = Queue::with_resources(&mut entries).unwrap();
                for (bit, bytes) in [(1, b"abc"), (2, b"def")] {
                    let w = messages.reserve(3).unwrap();
                    messages.write(&w).copy_from_slice(bytes);
                    let lease = messages.seal(w, 3).unwrap();
                    let offer = messages.offer(&lease).unwrap();
                    queue
                        .reserve_with(Resource { drops: &drops, bit })
                        .unwrap()
                        .accept(messages, offer);
                    messages.release(lease);
                }
                let mut writer = Writer {
                    attempts: 0,
                    received: 0,
                };
                let mut cx = Context::from_waker(Waker::noop());
                for _ in 0..16 {
                    if matches!(
                        io::poll_queue_resources(
                            &mut writer,
                            &mut queue,
                            messages,
                            &mut cx,
                            NonZeroUsize::new(1).unwrap()
                        ),
                        Poll::Ready(Ok(()))
                    ) {
                        break;
                    }
                    // Attachments stay alive after their first byte until the rest
                    // of this frame completes; the successor cannot overtake it.
                    if writer.attempts <= 5 {
                        assert_eq!(drops.get(), 0);
                    }
                }
                assert!(queue.is_empty());
                assert_eq!(writer.received, 3);
                assert_eq!(drops.get(), 3);
            },
        )
        .unwrap();
    });
    assert_eq!(usage.count_total, 0);
}

#[test]
fn resources_abort_and_terminal_discard_close_once_and_preserve_reader() {
    use core::cell::Cell;
    let drops = Cell::new(0);
    let mut bytes = [0; 3];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
        let mut entries = [None];
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        drop(
            queue
                .reserve_with(Resource {
                    drops: &drops,
                    bit: 1,
                })
                .unwrap(),
        );
        assert_eq!(drops.get(), 1);
        assert!(queue.is_empty());
        let w = messages.reserve(3).unwrap();
        messages.write(&w).copy_from_slice(b"abc");
        let lease = messages.seal(w, 3).unwrap();
        let offer = messages.offer(&lease).unwrap();
        let key = queue
            .reserve_with(Resource {
                drops: &drops,
                bit: 2,
            })
            .unwrap()
            .accept(messages, offer);
        assert!(matches!(
            queue.reserve_with(Resource {
                drops: &drops,
                bit: 4
            }),
            Err(Error::NoSendSlot)
        ));
        assert_eq!(drops.get(), 5);
        assert_eq!(
            queue.window(messages).unwrap().unwrap().advance(4),
            Err(Error::InvalidWriteCount)
        );
        assert!(queue
            .window(messages)
            .unwrap()
            .unwrap()
            .resources()
            .is_some());
        // Even an external advancement through the public send key uses the
        // same cursor: it cannot cause resources to be emitted a second time.
        messages.send_window(key).unwrap().advance(1).unwrap();
        assert!(queue
            .window(messages)
            .unwrap()
            .unwrap()
            .resources()
            .is_none());
        assert_eq!(queue.discard_one(messages), Err(Error::QueueStillLive));
        queue.fail();
        assert!(queue.discard_one(messages).unwrap());
        assert_eq!(drops.get(), 7);
        assert_eq!(&*messages.read(&lease), b"abc");
        messages.release(lease);
        assert!(matches!(
            queue.reserve_with(Resource {
                drops: &drops,
                bit: 8
            }),
            Err(Error::QueueFailed)
        ));
        assert_eq!(drops.get(), 15);
    })
    .unwrap();
}

#[test]
fn resource_io_failure_never_retries_transfer_or_exposes_successor() {
    struct Failure {
        step: usize,
        calls: usize,
        transferred: bool,
    }
    impl crate::io::Flush for Failure {
        type Error = u8;
        fn poll_flush(
            &mut self,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), crate::io::IoError<Self::Error>>> {
            core::task::Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<u8> for Failure {
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            _: &[u8],
            resource: Option<&u8>,
        ) -> Poll<Result<usize, IoError<u8>>> {
            self.calls += 1;
            if self.calls == self.step {
                return Poll::Ready(Err(IoError::Failed(7)));
            }
            if let Some(resource) = resource {
                assert_eq!(*resource, 41);
                assert!(!self.transferred);
                self.transferred = true;
            }
            Poll::Ready(Ok(1))
        }
    }
    for step in 1..=3 {
        let mut a = [0; 3];
        let mut b = [0; 3];
        let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
        with_messages(
            &mut buffers,
            &mut [const { SendSlot::EMPTY }; 2],
            |messages| {
                let mut entries = [const { None }; 2];
                let mut queue = Queue::with_resources(&mut entries).unwrap();
                for resource in [41, 43] {
                    let w = messages.reserve(3).unwrap();
                    let l = messages.seal(w, 3).unwrap();
                    let o = messages.offer(&l).unwrap();
                    queue.reserve_with(resource).unwrap().accept(messages, o);
                    messages.release(l);
                }
                let mut failure = Failure {
                    step,
                    calls: 0,
                    transferred: false,
                };
                let mut cx = Context::from_waker(Waker::noop());
                assert_eq!(
                    io::poll_queue_resources(
                        &mut failure,
                        &mut queue,
                        messages,
                        &mut cx,
                        NonZeroUsize::new(8).unwrap()
                    ),
                    Poll::Ready(Err(io::SendError::Io(7)))
                );
                assert!(queue.is_failed());
                assert_eq!(queue.len(), 2);
                assert_eq!(
                    io::poll_queue_resources(
                        &mut failure,
                        &mut queue,
                        messages,
                        &mut cx,
                        NonZeroUsize::new(8).unwrap()
                    ),
                    Poll::Ready(Err(io::SendError::State(Error::QueueFailed)))
                );
                assert_eq!(failure.calls, step);
                assert!(queue.discard_one(messages).unwrap());
                assert!(queue.discard_one(messages).unwrap());
            },
        )
        .unwrap();
    }
}

#[cfg(feature = "alloc")]
#[test]
fn growing_wrapped_fifo_moves_resource_owners_without_cloning() {
    use core::cell::Cell;
    let drops = Cell::new(0);
    let mut bytes = [[0; 1]; 4];
    let mut buffers = bytes.each_mut().map(|b| Buffer::new(b));
    with_messages(
        &mut buffers,
        &mut [const { SendSlot::EMPTY }; 4],
        |messages| {
            let mut queue = Queue::growing_with_resources(2, 4).unwrap();
            for bit in [1, 2, 4, 8] {
                let w = messages.reserve(1).unwrap();
                messages.write(&w)[0] = bit as u8;
                let l = messages.seal(w, 1).unwrap();
                let o = messages.offer(&l).unwrap();
                queue
                    .reserve_with(Resource { drops: &drops, bit })
                    .unwrap()
                    .accept(messages, o);
                messages.release(l);
                if bit == 2 {
                    queue.window(messages).unwrap().unwrap().advance(1).unwrap();
                }
            }
            assert_eq!(drops.get(), 1);
            for bit in [2, 4, 8] {
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.resources().unwrap().bit, bit);
                assert_eq!(window.bytes(), &[bit as u8]);
                if bit != 8 {
                    window.advance(1).unwrap();
                }
            }
            // Drop of caller/owned storage releases unsent resources too. Message
            // storage is finalized by its enclosing scope, never falsely completed.
            drop(queue);
            assert_eq!(drops.get(), 15);
        },
    )
    .unwrap();
}

#[test]
fn terminal_cleanup_of_stale_entry_preserves_reused_send_and_releases_resources() {
    let drops = core::cell::Cell::new(0);
    let mut bytes = [0; 3];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
        let mut entries = [None];
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        let writer = messages.reserve(3).unwrap();
        let lease = messages.seal(writer, 3).unwrap();
        let offer = messages.offer(&lease).unwrap();
        let old = queue
            .reserve_with(Resource {
                drops: &drops,
                bit: 1,
            })
            .unwrap()
            .accept(messages, offer);
        messages.release(lease);
        messages.fail_send(old).unwrap();
        let writer = messages.reserve(3).unwrap();
        messages.write(&writer).copy_from_slice(b"new");
        let lease = messages.seal(writer, 3).unwrap();
        let offer = messages.offer(&lease).unwrap();
        let current = messages.accept(offer);
        messages.release(lease);
        assert_ne!(old, current);
        assert!(matches!(queue.window(messages), Err(Error::StaleSend)));
        queue.fail();
        assert!(queue.discard_one(messages).unwrap());
        assert_eq!(drops.get(), 1);
        assert!(queue.is_empty());
        assert!(!queue.discard_one(messages).unwrap());
        let window = messages.send_window(current).unwrap();
        assert_eq!(window.bytes(), b"new");
        window.advance(3).unwrap();
    })
    .unwrap();
}
