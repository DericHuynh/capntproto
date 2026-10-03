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

type Message<M> = (M, oneshot::Sender<M>);
type Terminal = (Result<(), Error>, oneshot::Sender<()>);
struct Queue<M> {
    messages: VecDeque<Message<M>>,
    terminal: Option<Terminal>,
    accepting: bool,
    senders: usize,
}
struct Shared<M> {
    queue: Mutex<Queue<M>>,
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

enum Batch<M> {
    Messages(VecDeque<Message<M>>),
    Done(Option<Terminal>),
}
impl<M> Receiver<M> {
    async fn next(&self) -> Batch<M> {
        poll_fn(|cx| {
            self.0.ready.register(cx.waker());
            let mut queue = self.0.queue.lock().unwrap();
            if !queue.messages.is_empty() {
                let mut metrics = self.0.diagnostics.metrics.lock().unwrap();
                metrics.active = std::mem::take(&mut metrics.queued);
                Poll::Ready(Batch::Messages(std::mem::take(&mut queue.messages)))
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
        loop {
            match receiver.next().await {
                Batch::Messages(batch) => {
                    let (messages, completions): (Vec<_>, Vec<_>) = batch.into_iter().unzip();
                    crate::serialize::write_messages(&mut writer, &messages).await?;
                    writer.flush().await?;
                    receiver.0.diagnostics.metrics.lock().unwrap().active = BatchMetrics::default();
                    for (message, completion) in messages.into_iter().zip(completions) {
                        let _ = completion.send(message);
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
        let bytes = message
            .as_output_segments()
            .iter()
            .map(|s| s.len())
            .sum::<usize>();
        let now = (self.shared.diagnostics.clock)();
        let (complete, receipt) = oneshot::channel();
        {
            let mut queue = self.shared.queue.lock().unwrap();
            if queue.accepting {
                let mut metrics = self.shared.diagnostics.metrics.lock().unwrap();
                let metrics = &mut metrics.queued;
                metrics.count += 1;
                metrics.bytes += bytes;
                if metrics.oldest.is_none() {
                    metrics.oldest = Some(now);
                }
                queue.messages.push_back((message, complete));
            }
        }
        self.shared.ready.wake();
        receipt.map_err(|_| Error::disconnected("WriteQueue has terminated".into()))
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
