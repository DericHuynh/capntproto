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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use futures_util::{task::AtomicWaker, AsyncWrite, AsyncWriteExt, TryFutureExt};

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

type Message<M> = (M, Option<oneshot::Sender<M>>);
type Terminal = (Result<(), Error>, oneshot::Sender<()>);
struct Queue<M> {
    messages: VecDeque<Message<M>>,
    terminal: Option<Terminal>,
    accepting: bool,
    senders: usize,
}
struct Shared<M> {
    queue: Mutex<Queue<M>>,
    // A hint only: enqueue rechecks the oldest timestamp under the locks.
    // Later messages in a batch do not need another diagnostic clock sample.
    queued: AtomicBool,
    diagnostics: OutgoingQueue,
    ready: AtomicWaker,
}

/// A handle that allows messages to be sent to a write queue.
pub struct Sender<M: AsOutputSegments> {
    shared: Arc<Shared<M>>,
}
impl<M: AsOutputSegments> Clone for Sender<M> {
    fn clone(&self) -> Self {
        self.shared.queue.lock().unwrap().senders += 1;
        Self {
            shared: self.shared.clone(),
        }
    }
}
impl<M: AsOutputSegments> Drop for Sender<M> {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().senders -= 1;
        self.shared.ready.wake();
    }
}

// Created outside the async block so even an unpolled driver cleans up. User
// messages, completion senders and wakers are always dropped/woken outside locks.
struct Receiver<M>(Arc<Shared<M>>);
impl<M> Drop for Receiver<M> {
    fn drop(&mut self) {
        let discarded = {
            let mut queue = self.0.queue.lock().unwrap();
            queue.accepting = false;
            *self.0.diagnostics.metrics.lock().unwrap() = Metrics::default();
            (std::mem::take(&mut queue.messages), queue.terminal.take())
        };
        // A surviving Sender must not retain a canceled executor task.
        drop(self.0.ready.take());
        drop(discarded);
    }
}

enum Batch {
    Messages,
    Done(Option<Terminal>),
}
impl<M> Receiver<M> {
    async fn next(&self, batch: &mut VecDeque<Message<M>>) -> Batch {
        poll_fn(|cx| {
            self.0.ready.register(cx.waker());
            let mut queue = self.0.queue.lock().unwrap();
            if !queue.messages.is_empty() {
                let mut metrics = self.0.diagnostics.metrics.lock().unwrap();
                metrics.active = std::mem::take(&mut metrics.queued);
                // The drained batch lends its capacity to the next producers.
                // Both buffers stay owned until the driver is dropped.
                std::mem::swap(batch, &mut queue.messages);
                self.0.queued.store(false, Ordering::Relaxed);
                Poll::Ready(Batch::Messages)
            } else if queue.terminal.is_some() || !queue.accepting || queue.senders == 0 {
                queue.accepting = false;
                Poll::Ready(Batch::Done(queue.terminal.take()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

/// Creates a queue and its driver. Messages ready when a batch starts are written
/// together and flushed once. Dropping all senders drains the queue; dropping
/// the driver fails all outstanding sends. Poll the driver to make progress.
pub fn write_queue<W, M>(writer: W) -> (Sender<M>, impl Future<Output = Result<(), Error>>)
where
    W: AsyncWrite + Unpin,
    M: AsOutputSegments,
{
    let start = Instant::now();
    write_queue_with_clock(writer, move || start.elapsed())
}

/// Like [`write_queue`], using a caller-supplied monotonic clock for diagnostics.
/// The clock returns elapsed time from an arbitrary fixed origin.
pub fn write_queue_with_clock<W, M>(
    mut writer: W,
    clock: impl Fn() -> Duration + Send + Sync + 'static,
) -> (Sender<M>, impl Future<Output = Result<(), Error>>)
where
    W: AsyncWrite + Unpin,
    M: AsOutputSegments,
{
    let shared = Arc::new(Shared {
        queued: AtomicBool::new(false),
        queue: Mutex::new(Queue {
            messages: VecDeque::new(),
            terminal: None,
            accepting: true,
            senders: 1,
        }),
        diagnostics: OutgoingQueue {
            metrics: Arc::new(Mutex::new(Metrics::default())),
            clock: Arc::new(clock),
        },
        ready: AtomicWaker::new(),
    });
    let sender = Sender {
        shared: shared.clone(),
    };
    let receiver = Receiver(shared);
    let task = async move {
        let mut batch = VecDeque::new();
        loop {
            match receiver.next(&mut batch).await {
                Batch::Messages => {
                    crate::serialize::write_message_refs(
                        &mut writer,
                        batch.iter().map(|(message, _)| message),
                    )
                    .await?;
                    writer.flush().await?;
                    receiver.0.diagnostics.metrics.lock().unwrap().active = BatchMetrics::default();
                    for (message, completion) in batch.drain(..) {
                        if let Some(completion) = completion {
                            let _ = completion.send(message);
                        }
                    }
                    // Reuse ordinary batches without pinning a burst's peak
                    // metadata allocation for the lifetime of the connection.
                    if batch.capacity() > 1024 {
                        batch = VecDeque::new();
                    }
                }
                Batch::Done(terminal) => {
                    return match terminal {
                        Some((result, completion)) => {
                            let _ = completion.send(());
                            result
                        }
                        None => Ok(()),
                    };
                }
            }
        }
    };
    (sender, task)
}

impl<M: AsOutputSegments> Sender<M> {
    /// Enqueues synchronously. Resolves with the message after its batch is
    /// written and flushed. Dropping this receipt never cancels the write.
    pub fn send(&mut self, message: M) -> impl Future<Output = Result<M, Error>> + Unpin {
        let (complete, receipt) = oneshot::channel();
        self.enqueue(message, Some(complete));
        receipt.map_err(|_| Error::disconnected("WriteQueue has terminated".into()))
    }

    /// Enqueues without allocating a completion receipt. The driver owns the
    /// message until its batch is flushed, fails, or is canceled. Driver errors
    /// still propagate normally; use [`Self::send`] to observe this send alone.
    /// A stopped queue discards the message, just like dropping a failed receipt.
    pub fn send_detached(&mut self, message: M) {
        self.enqueue(message, None);
    }

    fn enqueue(&mut self, message: M, complete: Option<oneshot::Sender<M>>) {
        let bytes = message
            .as_output_segments()
            .iter()
            .map(|s| s.len())
            .sum::<usize>();
        let mut now = (!self.shared.queued.load(Ordering::Relaxed))
            .then(|| (self.shared.diagnostics.clock)());
        loop {
            let mut queue = self.shared.queue.lock().unwrap();
            if queue.accepting {
                let mut metrics = self.shared.diagnostics.metrics.lock().unwrap();
                if metrics.queued.oldest.is_none() && now.is_none() {
                    // The receiver may have started a batch since the hint.
                    // Sample outside both locks, including for reentrant clocks.
                    drop(metrics);
                    drop(queue);
                    now = Some((self.shared.diagnostics.clock)());
                    continue;
                }
                let metrics = &mut metrics.queued;
                metrics.count += 1;
                metrics.bytes += bytes;
                if metrics.oldest.is_none() {
                    metrics.oldest = now;
                }
                queue.messages.push_back((message, complete));
                self.shared.queued.store(true, Ordering::Relaxed);
            }
            break;
        }
        self.shared.ready.wake();
    }

    /// Observe pending messages without keeping the queue alive.
    pub fn outgoing_queue(&self) -> OutgoingQueue {
        self.shared.diagnostics.clone()
    }

    /// Number of messages waiting for the next batch, excluding the active batch.
    pub fn len(&self) -> usize {
        self.shared.diagnostics.metrics.lock().unwrap().queued.count
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Stops accepting new sends immediately, across all cloned senders. Drains
    /// accepted messages before resolving this receipt and completing the driver
    /// with `result`. Repeated termination requests fail. Dropping the receipt
    /// does not cancel shutdown. This does not close the underlying writer.
    pub fn terminate(
        &mut self,
        result: Result<(), Error>,
    ) -> impl Future<Output = Result<(), Error>> + Unpin {
        let (complete, receipt) = oneshot::channel();
        {
            let mut queue = self.shared.queue.lock().unwrap();
            if queue.accepting {
                queue.accepting = false;
                queue.terminal = Some((result, complete));
            }
        }
        self.shared.ready.wake();
        receipt.map_err(|_| Error::disconnected("WriteQueue has terminated".into()))
    }
}

fn _assert_kinds() {
    fn send<T: Send>(_: T) {}
    fn queue<W: AsyncWrite + Unpin + Send, M: AsOutputSegments + Sync + Send>(w: W) {
        let (s, f) = write_queue::<W, M>(w);
        send(s);
        send(f);
    }
    fn builder<W: AsyncWrite + Unpin + Send>(w: W) {
        let (s, f) = write_queue::<W, capnp::message::Builder<capnp::message::HeapAllocator>>(w);
        send(s);
        send(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::message::{Builder, HeapAllocator};
    use futures::FutureExt;
    use std::sync::atomic::AtomicU64;

    fn message() -> Builder<HeapAllocator> {
        let mut message = Builder::new_default();
        message.initn_root::<capnp::data::Builder>(0);
        message
    }

    #[test]
    fn stopped_queue_discards_new_messages_and_fails_outstanding_receipts() {
        let (mut sender, driver) = write_queue(futures::io::sink());
        let diagnostics = sender.outgoing_queue();
        let accepted = sender.send(message());
        assert_eq!(sender.len(), 1);
        drop(driver);
        assert!(accepted.now_or_never().unwrap().is_err());

        sender.send_detached(message());
        assert!(sender.send(message()).now_or_never().unwrap().is_err());
        assert!(sender.is_empty());
        assert_eq!(diagnostics.output_snapshot(), OutputSnapshot::default());
    }

    #[test]
    fn oversized_batch_flushes_every_receipt_and_releases_capacity_for_later_work() {
        let (mut sender, driver) = write_queue(futures::io::sink());
        let receipts: Vec<_> = (0..1025).map(|_| sender.send(message())).collect();
        futures::pin_mut!(driver);
        assert!(driver.as_mut().now_or_never().is_none());
        for receipt in receipts {
            receipt.now_or_never().unwrap().unwrap();
        }
        let later = sender.send(message());
        assert!(driver.as_mut().now_or_never().is_none());
        later.now_or_never().unwrap().unwrap();
        assert_eq!(sender.outgoing_queue().snapshot().message_count, 0);
        assert!(sender.shared.queue.lock().unwrap().messages.capacity() <= 1024);
        drop(sender);
        driver.now_or_never().unwrap().unwrap();
    }

    #[test]
    fn one_enqueue_clock_sample_per_batch_preserves_oldest_age() {
        let ticks = Arc::new(AtomicU64::new(0));
        let clock = ticks.clone();
        let (mut sender, driver) = write_queue_with_clock(futures::io::sink(), move || {
            Duration::from_secs(clock.fetch_add(1, Ordering::Relaxed))
        });
        futures::pin_mut!(driver);
        for _ in 0..3 {
            sender.send_detached(message());
        }
        assert_eq!(ticks.load(Ordering::Relaxed), 1);
        let snapshot = sender.outgoing_queue().snapshot();
        assert_eq!(snapshot.message_count, 3);
        assert_eq!(snapshot.wait_time, Duration::from_secs(1));
        assert!(driver.as_mut().now_or_never().is_none());
        sender.send_detached(message());
        assert_eq!(ticks.load(Ordering::Relaxed), 3);
        let snapshot = sender.outgoing_queue().snapshot();
        assert_eq!(snapshot.message_count, 1);
        assert_eq!(snapshot.wait_time, Duration::from_secs(1));
    }

    #[test]
    fn stale_hint_resamples_outside_locks_after_a_concurrent_batch_start() {
        type Message = Builder<HeapAllocator>;
        let shared = Arc::new(Mutex::new(None::<std::sync::Weak<Shared<Message>>>));
        let observed = shared.clone();
        let (mut sender, _driver) = write_queue_with_clock(futures::io::sink(), move || {
            let shared = observed
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap();
            assert!(
                shared.queue.try_lock().is_ok(),
                "clock ran under queue lock"
            );
            assert!(
                shared.diagnostics.metrics.try_lock().is_ok(),
                "clock ran under metrics lock"
            );
            Duration::from_secs(17)
        });
        *shared.lock().unwrap() = Some(Arc::downgrade(&sender.shared));
        // Model a producer observing the old hint just as the receiver drains
        // the queue. The locked state, not the hint, determines the timestamp.
        sender.shared.queued.store(true, Ordering::Relaxed);
        sender.send_detached(message());
        assert_eq!(sender.len(), 1);
        assert_eq!(
            sender
                .shared
                .diagnostics
                .metrics
                .lock()
                .unwrap()
                .queued
                .oldest,
            Some(Duration::from_secs(17))
        );
    }

    #[test]
    fn reentrant_clock_can_enqueue_without_changing_the_oldest_timestamp() {
        type Message = Builder<HeapAllocator>;
        let access = Arc::new(Mutex::new(None::<Sender<Message>>));
        let weak_access = Arc::downgrade(&access);
        let entered = AtomicBool::new(false);
        let (mut sender, driver) = write_queue_with_clock(futures::io::sink(), move || {
            if !entered.swap(true, Ordering::Relaxed) {
                let access = weak_access.upgrade().unwrap();
                let mut sender = access.lock().unwrap().as_ref().unwrap().clone();
                sender.send_detached(message());
            }
            Duration::from_secs(17)
        });
        *access.lock().unwrap() = Some(sender.clone());
        sender.send_detached(message());
        assert_eq!(sender.len(), 2);
        assert_eq!(sender.outgoing_queue().snapshot().wait_time, Duration::ZERO);
        futures::pin_mut!(driver);
        assert!(driver.as_mut().now_or_never().is_none());
    }
}
