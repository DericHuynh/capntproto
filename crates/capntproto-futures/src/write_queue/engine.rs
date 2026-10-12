// Shared FIFO and flush-watermark implementation for both synchronization modes.
type Message<M> = (M, Option<oneshot::Sender<M>>);
type Terminal = (Result<(), Error>, oneshot::Sender<()>);
struct Queue<M> {
    messages: VecDeque<Message<M>>,
    terminal: Option<Terminal>,
    accepting: bool,
    senders: usize,
    accepted: u64,
    flushed: u64,
    stopped: bool,
}
struct Shared<M> {
    queue: QueueCell<Queue<M>>,
    // A hint only: enqueue rechecks the oldest timestamp under the locks.
    // Later messages in a batch do not need another diagnostic clock sample.
    queued: Flag,
    diagnostics: OutgoingQueue,
    ready: Notify,
    observing: Flag,
    flushed: Notify,
}

/// A handle that allows messages to be sent to a write queue.
pub struct Sender<M: AsOutputSegments> {
    shared: Owner<Shared<M>>,
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
struct Receiver<M>(Owner<Shared<M>>);
impl<M> Drop for Receiver<M> {
    fn drop(&mut self) {
        let discarded = {
            let mut queue = self.0.queue.lock().unwrap();
            queue.accepting = false;
            queue.stopped = true;
            *self.0.diagnostics.metrics.lock().unwrap() = Metrics::default();
            (std::mem::take(&mut queue.messages), queue.terminal.take())
        };
        // A surviving Sender must not retain a canceled executor task.
        drop(self.0.ready.take());
        self.0.flushed.wake();
        drop(discarded);
    }
}

enum Batch {
    Messages(u64),
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
                Poll::Ready(Batch::Messages(queue.accepted))
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
    let shared = Owner::new(Shared {
        queued: Flag::new(false),
        queue: QueueCell::new(Queue {
            messages: VecDeque::new(),
            terminal: None,
            accepting: true,
            senders: 1,
            accepted: 0,
            flushed: 0,
            stopped: false,
        }),
        diagnostics: OutgoingQueue {
            metrics: Arc::new(Mutex::new(Metrics::default())),
            clock: Arc::new(clock),
        },
        ready: Notify::new(),
        observing: Flag::new(false),
        flushed: Notify::new(),
    });
    let sender = Sender {
        shared: shared.clone(),
    };
    let receiver = Receiver(shared);
    let task = async move {
        let mut batch = VecDeque::new();
        loop {
            match receiver.next(&mut batch).await {
                Batch::Messages(through) => {
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
                    receiver.0.queue.lock().unwrap().flushed = through;
                    receiver.0.flushed.wake();
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
    pub fn send(&self, message: M) -> impl Future<Output = Result<M, Error>> + Unpin {
        let (complete, receipt) = oneshot::channel();
        let _ = self.enqueue(message, Some(complete), true);
        receipt.map_err(|_| Error::disconnected("WriteQueue has terminated".into()))
    }

    /// Enqueues without allocating a completion receipt. The driver owns the
    /// message until its batch is flushed, fails, or is canceled. Driver errors
    /// still propagate normally; use [`Self::send`] to observe this send alone.
    /// A stopped queue discards the message, just like dropping a failed receipt.
    pub fn send_detached(&self, message: M) {
        let _ = self.enqueue(message, None, true);
    }

    /// Enqueue without a receipt and offer the existing driver an immediate
    /// poll. Normal notification follows if it cannot consume the queue.
    /// The callback and its unwinding run outside queue/diagnostics locks.
    pub fn send_detached_and_drive(&self, message: M, drive: impl FnOnce()) -> Result<(), Error> {
        self.enqueue(message, None, false)?;
        self.drive_queued(drive);
        Ok(())
    }

    fn drive_queued(&self, drive: impl FnOnce()) {
        struct NotifyQueued<'a, M>(&'a Shared<M>);
        impl<M> Drop for NotifyQueued<'_, M> {
            fn drop(&mut self) {
                let queued = !self.0.queue.lock().unwrap().messages.is_empty();
                if queued {
                    self.0.ready.wake();
                }
            }
        }
        let _notify = NotifyQueued(&self.shared);
        drive();
    }

    fn enqueue(
        &self,
        message: M,
        complete: Option<oneshot::Sender<M>>,
        notify: bool,
    ) -> Result<u64, Error> {
        let bytes = message.size_in_bytes();
        let mut now = (!self.shared.queued.load(Ordering::Relaxed))
            .then(|| (self.shared.diagnostics.clock)());
        let accepted = loop {
            let mut queue = self.shared.queue.lock().unwrap();
            if queue.accepting {
                let next = queue
                    .accepted
                    .checked_add(1)
                    .ok_or_else(|| Error::overloaded("write sequence exhausted".into()))?;
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
                queue.accepted = next;
                self.shared.queued.store(true, Ordering::Relaxed);
                break next;
            }
            return Err(Error::disconnected("WriteQueue has terminated".into()));
        };
        if notify {
            self.shared.ready.wake();
        }
        Ok(accepted)
    }

    /// Claim the queue's single cumulative flush observer. It shares FIFO order
    /// with ordinary sends but allocates no individual message receipts. A new
    /// observer may be claimed after the old one is dropped.
    pub fn observe(&self) -> Option<ObservedSender<M>> {
        if self
            .shared
            .observing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        Some(ObservedSender {
            sender: self.clone(),
            through: 0,
        })
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
        &self,
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

/// An ordered batch publisher with one flush watermark. Send enqueues immediately;
/// poll_flush waits until every message sent through this handle has physically
/// flushed. Dropping this handle never cancels accepted output.
pub struct ObservedSender<M: AsOutputSegments> {
    sender: Sender<M>,
    through: u64,
}
impl<M: AsOutputSegments> ObservedSender<M> {
    pub fn send(&mut self, message: M) -> Result<(), Error> {
        self.through = self.sender.enqueue(message, None, true)?;
        Ok(())
    }
    /// Offer the driver an immediate poll before scheduling it. If the callback
    /// does not consume the queue, normal notification follows, including during
    /// unwinding. A consumed batch has registered its IO or next-queue wake.
    /// The callback runs without queue or diagnostics locks held.
    pub fn send_and_drive(&mut self, message: M, drive: impl FnOnce()) -> Result<(), Error> {
        // Commit the watermark before calling the driver, including when the
        // callback unwinds after accepting a frame but before flushing it.
        self.through = self.sender.enqueue(message, None, false)?;
        self.sender.drive_queued(drive);
        Ok(())
    }
    pub fn poll_flush(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Error>> {
        let status = || {
            let queue = self.sender.shared.queue.lock().unwrap();
            if queue.flushed >= self.through {
                Poll::Ready(Ok(()))
            } else if queue.stopped {
                Poll::Ready(Err(Error::disconnected("WriteQueue has terminated".into())))
            } else {
                Poll::Pending
            }
        };
        let mut result = status();
        if result.is_pending() {
            self.sender.shared.flushed.register(cx.waker());
            // Register before rechecking to cover a concurrent physical flush.
            result = status();
        }
        if result.is_ready() {
            drop(self.sender.shared.flushed.take());
        }
        result
    }
}
impl<M: AsOutputSegments> Drop for ObservedSender<M> {
    fn drop(&mut self) {
        let wake = self.sender.shared.flushed.take();
        self.sender.shared.observing.store(false, Ordering::Release);
        drop(wake);
    }
}
