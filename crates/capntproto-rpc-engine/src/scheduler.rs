//! Legacy scheduling policy over borrowed storage. Ownership providers supply
//! admission, ready tasks, wake leases and empty notifications; there is one
//! ordinary/deferred/direct-connection poll loop for both profiles.
use core::{
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

pub mod bounded;
#[cfg(test)]
mod tests;

pub enum Command<F, E, O> {
    Task(F),
    Last(F),
    Connection(F),
    Terminate(Result<(), E>),
    OnEmpty(O),
}
pub enum Done<E> {
    Continue(Result<(), E>),
    Terminate(Result<(), E>),
}

/// Capacity is checked before first polling a submitted future. A bounded
/// source retains a blocked command without executing its protocol effects.
#[derive(Clone, Copy)]
pub struct Capacity {
    pub regular: bool,
    pub deferred: bool,
    pub connection: bool,
    pub empty: bool,
}
pub type Next<A> =
    Poll<Option<Command<<A as Source>::Task, <A as Source>::Error, <A as Source>::Empty>>>;
pub trait Source {
    type Error;
    type Task: Future<Output = Result<(), Self::Error>> + Unpin;
    type Empty;
    fn poll_next(&mut self, cx: &mut Context<'_>, capacity: Capacity) -> Next<Self>;
    fn is_empty(&self) -> bool;
    fn is_closed(&self) -> bool;
    fn close(&mut self);
    /// Bounded control can bypass a full work queue. Hosted admission retains
    /// its existing FIFO termination marker instead.
    fn poll_stop(&mut self, _cx: &mut Context<'_>) -> Option<Result<(), Self::Error>> {
        None
    }
    fn capacity_released(&mut self, _cx: &mut Context<'_>, _capacity: Capacity) {}
}
pub trait Work<F, E> {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn has_capacity(&self) -> bool;
    /// The source must first accept the corresponding capacity credit.
    fn push(&mut self, future: F);
    /// Termination storage must be independent of ordinary work capacity.
    fn terminate(&mut self, result: Result<(), E>);
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Done<E>>>;
}
pub trait Lease {
    fn waker(&self) -> impl Deref<Target = Waker>;
}
pub trait Parent {
    type Lease: Lease;
    fn register(&self, parent: &Waker);
    fn retain(&self) -> Self::Lease;
}
pub struct Signal<'a, P> {
    pub ready: &'a AtomicBool,
    pub parent: &'a P,
}
pub trait Empty<O> {
    fn is_empty(&self) -> bool;
    fn has_capacity(&self) -> bool;
    fn push(&mut self, notification: O);
    fn fulfill(&mut self);
    fn cancel(&mut self);
}
pub trait Reaper<E> {
    fn task_succeeded(&mut self) {}
    fn task_failed(&mut self, error: E);
}

/// Ephemeral projections preserve the hosted driver's field ownership and
/// layout. `idle_regular` is its existing sentinel count, or zero for storage
/// that permits empty/restarted task sets. Wake providers must register before
/// polling and retain valid wake identities across all user callbacks.
pub struct Parts<'a, A: Source, W, P, O, R: ?Sized> {
    pub enqueued: &'a mut A,
    pub connection: &'a mut Option<A::Task>,
    pub in_progress: &'a mut W,
    pub last: &'a mut W,
    pub activity: Signal<'a, P>,
    pub last_activity: Signal<'a, P>,
    pub deferred_last: &'a mut bool,
    pub busy_turns: &'a mut u8,
    pub defer: fn(&mut Context<'_>),
    pub on_empty_fulfillers: &'a mut O,
    pub reaper: &'a mut R,
    pub idle_regular: usize,
}
impl<A, W, P, O, R> Parts<'_, A, W, P, O, R>
where
    A: Source,
    W: Work<A::Task, A::Error>,
    P: Parent,
    O: Empty<A::Empty>,
    R: Reaper<A::Error> + ?Sized,
{
    #[inline(always)]
    fn capacity(&self) -> Capacity {
        Capacity {
            regular: self.in_progress.has_capacity(),
            deferred: self.last.has_capacity(),
            connection: self.connection.is_none(),
            empty: self.on_empty_fulfillers.has_capacity(),
        }
    }
    #[inline(always)]
    fn finished(&self) -> bool {
        self.enqueued.is_closed()
            && self.connection.is_none()
            && self.in_progress.len() <= self.idle_regular
            && self.last.len() == 0
    }
    #[inline(always)]
    fn update_on_empty_fulfillers(&mut self) {
        if self.on_empty_fulfillers.is_empty() {
            return;
        }
        if self.connection.is_none()
            && self.in_progress.len() <= self.idle_regular
            && self.last.len() == 0
            && self.enqueued.is_empty()
        {
            self.on_empty_fulfillers.fulfill();
        }
    }
    #[inline(always)]
    fn reap(&mut self, result: Result<(), A::Error>) {
        match result {
            Ok(()) => self.reaper.task_succeeded(),
            Err(error) => self.reaper.task_failed(error),
        }
    }
    #[inline(always)]
    fn released(&mut self, cx: &mut Context<'_>) {
        self.enqueued.capacity_released(cx, self.capacity());
    }
    #[inline(always)]
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), A::Error>> {
        self.activity.parent.register(cx.waker());
        self.last_activity.parent.register(cx.waker());
        if let Some(result) = self.enqueued.poll_stop(cx) {
            self.on_empty_fulfillers.cancel();
            return Poll::Ready(result);
        }
        let notified = self.activity.ready.swap(false, Ordering::AcqRel);
        let activity = self.activity.parent.retain();
        let wake = activity.waker();
        if let Poll::Ready(result) = self.poll_regular(&mut Context::from_waker(&wake)) {
            return Poll::Ready(result);
        }
        if self.last.len() == 0 || !self.last_activity.ready.load(Ordering::Acquire) {
            return Poll::Pending;
        }
        let busy =
            notified || self.activity.ready.load(Ordering::Acquire) || !self.enqueued.is_empty();
        // An external response consumer gets a turn before publication, while
        // the bounded delay prevents a ready producer starving transport IO.
        if (busy && *self.busy_turns < 32) || !*self.deferred_last {
            *self.busy_turns = self.busy_turns.saturating_add(1);
            *self.deferred_last = true;
            (self.defer)(cx);
            return Poll::Pending;
        }
        *self.busy_turns = 0;
        *self.deferred_last = false;
        self.last_activity.ready.store(false, Ordering::Release);
        let last_activity = self.last_activity.parent.retain();
        let last_wake = last_activity.waker();
        let mut last_cx = Context::from_waker(&last_wake);
        while let Poll::Ready(Some(done)) = self.last.poll_next(&mut last_cx) {
            match done {
                Done::Continue(result) => self.reap(result),
                Done::Terminate(result) => return Poll::Ready(result),
            }
            self.update_on_empty_fulfillers();
            self.released(cx);
        }
        if self.finished() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
    #[inline]
    fn poll_regular(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), A::Error>> {
        let mut enqueued_stream_complete = false;
        for admission in 0..32 {
            if self.enqueued.is_closed() {
                break;
            }
            match self.enqueued.poll_next(cx, self.capacity()) {
                Poll::Pending => break,
                Poll::Ready(None) => {
                    enqueued_stream_complete = true;
                    break;
                }
                Poll::Ready(Some(Command::Terminate(result))) => {
                    self.in_progress.terminate(result);
                    break;
                }
                Poll::Ready(Some(Command::Task(mut future))) => {
                    match Pin::new(&mut future).poll(cx) {
                        Poll::Ready(result) => {
                            drop(future);
                            self.reap(result);
                        }
                        Poll::Pending => self.in_progress.push(future),
                    }
                }
                Poll::Ready(Some(Command::Last(future))) => {
                    self.last.push(future);
                    self.last_activity.ready.store(true, Ordering::Release);
                }
                Poll::Ready(Some(Command::Connection(future))) => {
                    if self.connection.is_none() {
                        *self.connection = Some(future);
                    } else {
                        self.in_progress.push(future);
                    }
                }
                Poll::Ready(Some(Command::OnEmpty(notification))) => {
                    self.on_empty_fulfillers.push(notification);
                }
            }
            if admission == 31 {
                cx.waker().wake_by_ref();
            }
        }
        if enqueued_stream_complete {
            self.enqueued.close();
        }
        if let Some(future) = self.connection.as_mut() {
            if let Poll::Ready(result) = Pin::new(future).poll(cx) {
                drop(self.connection.take());
                self.reap(result);
            }
        }
        self.update_on_empty_fulfillers();
        self.released(cx);
        if self.finished() {
            return Poll::Ready(Ok(()));
        }
        loop {
            match self.in_progress.poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Ready(Some(Done::Continue(result))) => {
                    self.reap(result);
                    self.update_on_empty_fulfillers();
                    self.released(cx);
                    if self.finished() {
                        return Poll::Ready(Ok(()));
                    }
                }
                Poll::Ready(Some(Done::Terminate(result))) => {
                    self.on_empty_fulfillers.cancel();
                    return Poll::Ready(result);
                }
            }
        }
    }
}
