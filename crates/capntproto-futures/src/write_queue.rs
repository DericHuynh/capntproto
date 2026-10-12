// Copyright (c) 2016 Sandstorm Development Group, Inc. and contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use std::collections::VecDeque;
use std::future::{poll_fn, Future};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use futures_util::{AsyncWrite, AsyncWriteExt, TryFutureExt};

use crate::serialize::AsOutputSegments;
use capnp::Error;

/// A snapshot of messages waiting for the next write batch. An active batch is
/// excluded, even while its writer is blocked. Bytes exclude stream framing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub message_count: usize,
    pub bytes: usize,
    pub wait_time: Duration,
}

/// Output still owned by the writer. Ages start at enqueue time; bytes exclude
/// framing and count whole messages, including bytes already partially written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub queued: QueueSnapshot,
    /// The batch being written or flushed. Cleared only after flush succeeds,
    /// or when the driver fails or is dropped.
    pub active: QueueSnapshot,
}

#[derive(Default)]
struct BatchMetrics {
    count: usize,
    bytes: usize,
    oldest: Option<Duration>,
}

impl BatchMetrics {
    fn snapshot(&self, now: Duration) -> QueueSnapshot {
        QueueSnapshot {
            message_count: self.count,
            bytes: self.bytes,
            wait_time: self
                .oldest
                .map_or(Duration::ZERO, |t| now.saturating_sub(t)),
        }
    }
}

#[derive(Default)]
struct Metrics {
    queued: BatchMetrics,
    active: BatchMetrics,
}

/// Cloneable diagnostics which do not keep the queue, writer or connection alive.
#[derive(Clone)]
pub struct OutgoingQueue {
    metrics: Arc<Mutex<Metrics>>,
    clock: Arc<dyn Fn() -> Duration + Send + Sync>,
}
impl OutgoingQueue {
    pub fn snapshot(&self) -> QueueSnapshot {
        self.output_snapshot().queued
    }

    pub fn output_snapshot(&self) -> OutputSnapshot {
        // A user-provided clock must never run under the metrics lock.
        let now = (self.clock)();
        let metrics = self.metrics.lock().unwrap();
        OutputSnapshot {
            queued: metrics.queued.snapshot(now),
            active: metrics.active.snapshot(now),
        }
    }
}

mod concurrent {
    use super::*;
    use futures_util::task::AtomicWaker as Notify;
    use std::sync::{atomic::AtomicBool as Flag, Arc as Owner, Mutex as QueueCell};
    include!("write_queue/engine.rs");
    fn _assert_kinds() {
        fn send<T: Send>(_: T) {}
        fn queue<W: AsyncWrite + Unpin + Send, M: AsOutputSegments + Sync + Send>(w: W) {
            let (s, f) = write_queue::<W, M>(w);
            send(s);
            send(f);
        }
        fn builder<W: AsyncWrite + Unpin + Send>(w: W) {
            let (s, f) =
                write_queue::<W, capnp::message::Builder<capnp::message::HeapAllocator>>(w);
            send(s);
            send(f);
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::atomic::AtomicBool;
        include!("write_queue/common_tests.rs");
        include!("write_queue/concurrent_tests.rs");
    }
}
mod local {
    use super::local_sync::{Flag, Notify, QueueCell};
    use super::*;
    use std::rc::Rc as Owner;
    include!("write_queue/engine.rs");
    #[cfg(test)]
    mod tests {
        include!("write_queue/common_tests.rs");

        #[test]
        fn local_clock_can_reenter_without_borrowing_the_queue() {
            use std::cell::RefCell;
            thread_local! {
                static ACCESS: RefCell<Option<Sender<Builder<HeapAllocator>>>> = const { RefCell::new(None) };
            }
            let entered = std::sync::atomic::AtomicBool::new(false);
            let (sender, driver) = write_queue_with_clock(futures::io::sink(), move || {
                if !entered.swap(true, Ordering::Relaxed) {
                    let sender = ACCESS.with(|access| access.borrow().as_ref().unwrap().clone());
                    sender.send_detached(message());
                }
                Duration::from_secs(17)
            });
            ACCESS.with(|access| *access.borrow_mut() = Some(sender.clone()));
            sender.send_detached(message());
            ACCESS.with(|access| {
                access.borrow_mut().take();
            });
            assert_eq!(sender.len(), 2);
            assert_eq!(sender.outgoing_queue().snapshot().wait_time, Duration::ZERO);
            futures::pin_mut!(driver);
            assert!(driver.as_mut().now_or_never().is_none());
            assert!(sender.is_empty());
        }
    }
}
mod local_sync;
pub use concurrent::{write_queue, write_queue_with_clock, ObservedSender, Sender};
pub use local::{
    write_queue as write_queue_local, write_queue_with_clock as write_queue_local_with_clock,
    ObservedSender as LocalObservedSender, Sender as LocalSender,
};
