//! Fixed providers for the shared legacy scheduler. Commands remain owned and
//! unpolled until their storage credit is available. Stop uses an independent
//! caller-owned completion slot, so saturated ordinary work cannot block it.
use super::*;
use crate::{admission, admission::Buffer, completion, tasks};

type Input<'a, F, E> = Command<F, E, completion::Sender<'a, ()>>;

pub struct Inbox<'queue, 'control, 'reply, F, E, const N: usize> {
    receiver: Option<admission::BoundedReceiver<'queue, Input<'reply, F, E>, N>>,
    stop: Option<completion::Receiver<'control, Result<(), E>>>,
    pending: Option<Input<'reply, F, E>>,
    blocked: bool,
}
impl<'queue, 'control, 'reply, F, E, const N: usize> Inbox<'queue, 'control, 'reply, F, E, N> {
    pub fn new(
        receiver: admission::BoundedReceiver<'queue, Input<'reply, F, E>, N>,
        stop: completion::Receiver<'control, Result<(), E>>,
    ) -> Self {
        Self {
            receiver: Some(receiver),
            stop: Some(stop),
            pending: None,
            blocked: false,
        }
    }
    fn accepts(command: &Input<'_, F, E>, capacity: Capacity) -> bool {
        match command {
            Command::Task(_) => capacity.regular,
            Command::Last(_) => capacity.deferred,
            Command::Connection(_) => capacity.connection || capacity.regular,
            Command::OnEmpty(_) => capacity.empty,
            Command::Terminate(_) => true,
        }
    }
}
impl<'reply, F, E, const N: usize> Source for Inbox<'_, '_, 'reply, F, E, N>
where
    F: Future<Output = Result<(), E>> + Unpin,
{
    type Error = E;
    type Task = F;
    type Empty = completion::Sender<'reply, ()>;
    fn poll_next(
        &mut self,
        cx: &mut Context<'_>,
        capacity: Capacity,
    ) -> Poll<Option<Input<'reply, F, E>>> {
        let command = if let Some(command) = self.pending.take() {
            command
        } else if let Some(receiver) = &mut self.receiver {
            match receiver.poll_next(cx) {
                Poll::Ready(Some(command)) => command,
                other => return other,
            }
        } else {
            return Poll::Ready(None);
        };
        if !Self::accepts(&command, capacity) {
            self.pending = Some(command);
            self.blocked = true;
            return Poll::Pending;
        }
        self.blocked = false;
        Poll::Ready(Some(command))
    }
    fn is_empty(&self) -> bool {
        self.pending.is_none() && self.receiver.as_ref().is_none_or(|r| r.is_empty())
    }
    fn is_closed(&self) -> bool {
        self.receiver.is_none()
    }
    fn close(&mut self) {
        drop(self.receiver.take());
        drop(self.pending.take());
    }
    fn poll_stop(&mut self, cx: &mut Context<'_>) -> Option<Result<(), E>> {
        let result = Pin::new(self.stop.as_mut()?).poll(cx);
        match result {
            Poll::Pending => None,
            Poll::Ready(result) => {
                drop(self.stop.take());
                result.ok()
            }
        }
    }
    fn capacity_released(&mut self, cx: &mut Context<'_>, capacity: Capacity) {
        if self.blocked
            && self
                .pending
                .as_ref()
                .is_some_and(|c| Self::accepts(c, capacity))
        {
            self.blocked = false;
            cx.waker().wake_by_ref();
        }
    }
}

/// Ready work and an independent termination marker, in caller-pinned storage.
pub struct Tasks<'a, F: Future<Output = Result<(), E>>, E, const N: usize> {
    pub tasks: tasks::Set<'a, F, N>,
    terminal: Option<Result<(), E>>,
}
impl<'a, F: Future<Output = Result<(), E>>, E, const N: usize> Tasks<'a, F, E, N> {
    pub fn new(tasks: tasks::Set<'a, F, N>) -> Self {
        Self {
            tasks,
            terminal: None,
        }
    }
}
impl<F: Future<Output = Result<(), E>>, E, const N: usize> Work<F, E> for Tasks<'_, F, E, N> {
    fn len(&self) -> usize {
        self.tasks.len() + usize::from(self.terminal.is_some())
    }
    fn has_capacity(&self) -> bool {
        self.tasks.len() < N
    }
    fn push(&mut self, future: F) {
        assert!(self.tasks.try_push(future, || {}).is_ok(), "missing credit");
    }
    fn terminate(&mut self, result: Result<(), E>) {
        assert!(self.terminal.is_none(), "termination already admitted");
        self.terminal = Some(result);
    }
    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Done<E>>> {
        if let Some(result) = self.terminal.take() {
            return Poll::Ready(Some(Done::Terminate(result)));
        }
        match self.tasks.poll_next(cx) {
            Poll::Ready(Some(result)) => Poll::Ready(Some(Done::Continue(result))),
            // The scheduler's source still owns restart/termination policy.
            Poll::Pending | Poll::Ready(None) => Poll::Pending,
        }
    }
}

/// The callback registers the outer executor's parent waker if needed. Its
/// allocation policy belongs to the caller. Child identities and `wake` must
/// notify the corresponding activity flag and arrange another outer poll.
pub struct StaticParent<'a> {
    wake: &'a Waker,
    register: fn(&Waker),
}
impl<'a> StaticParent<'a> {
    pub fn new(wake: &'a Waker, register: fn(&Waker)) -> Self {
        Self { wake, register }
    }
}
impl Lease for &Waker {
    fn waker(&self) -> impl Deref<Target = Waker> {
        *self
    }
}
impl<'a> Parent for StaticParent<'a> {
    type Lease = &'a Waker;
    fn register(&self, parent: &Waker) {
        (self.register)(parent);
    }
    fn retain(&self) -> Self::Lease {
        self.wake
    }
}

pub struct Notifications<'a, const N: usize> {
    pending: admission::Ring<completion::Sender<'a, ()>, N>,
}
impl<const N: usize> Default for Notifications<'_, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> Notifications<'_, N> {
    pub const fn new() -> Self {
        Self {
            pending: admission::Ring::new(),
        }
    }
}
impl<'a, const N: usize> Empty<completion::Sender<'a, ()>> for Notifications<'a, N> {
    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    fn has_capacity(&self) -> bool {
        !self.pending.is_full()
    }
    fn push(&mut self, notification: completion::Sender<'a, ()>) {
        assert!(self.pending.push(notification).is_ok(), "missing credit");
    }
    fn fulfill(&mut self) {
        while let Some(notification) = self.pending.pop() {
            let _ = notification.send(());
        }
    }
    fn cancel(&mut self) {
        while let Some(notification) = self.pending.pop() {
            drop(notification);
        }
    }
}
