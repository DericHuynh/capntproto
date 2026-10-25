//! Local task admission with borrowed or shared ownership. Queue capacity
//! rejection returns the input unchanged. Wakers and user values leave state
//! borrows before waking or destruction can reenter the channel.
use core::{
    cell::RefCell,
    task::{Context, Poll, Waker},
};

mod sealed {
    pub trait Buffer {}
    pub trait Owner {}
}

/// Storage operations used by the channel. Implementations are sealed so
/// queue operations cannot invoke user destructors while state is borrowed.
#[doc(hidden)]
pub trait Buffer: sealed::Buffer {
    type Item;
    fn push(&mut self, value: Self::Item) -> Result<(), Self::Item>;
    fn pop(&mut self) -> Option<Self::Item>;
    fn is_empty(&self) -> bool;
    fn retire(&mut self) -> Option<Self>
    where
        Self: Sized;
}

/// Fixed FIFO backing; a full queue preserves ownership of the rejected value.
pub struct Ring<T, const N: usize> {
    slots: [Option<T>; N],
    head: usize,
    len: usize,
}
impl<T, const N: usize> Ring<T, N> {
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; N],
            head: 0,
            len: 0,
        }
    }
}
impl<T, const N: usize> Default for Ring<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T, const N: usize> sealed::Buffer for Ring<T, N> {}
impl<T, const N: usize> Buffer for Ring<T, N> {
    type Item = T;
    #[inline]
    fn push(&mut self, value: T) -> Result<(), T> {
        if self.len == N {
            return Err(value);
        }
        // Avoid head + len overflow even for a very large caller array.
        let tail = if self.len >= N - self.head {
            self.len - (N - self.head)
        } else {
            self.head + self.len
        };
        self.slots[tail] = Some(value);
        self.len += 1;
        Ok(())
    }
    #[inline]
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let value = self.slots[self.head].take();
        self.head = if self.head + 1 == N { 0 } else { self.head + 1 };
        self.len -= 1;
        value
    }
    #[inline]
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline]
    fn retire(&mut self) -> Option<Self> {
        None
    }
}

#[cfg(feature = "alloc")]
impl<T> sealed::Buffer for alloc::collections::VecDeque<T> {}
#[cfg(feature = "alloc")]
impl<T> Buffer for alloc::collections::VecDeque<T> {
    type Item = T;
    #[inline(always)]
    fn push(&mut self, value: T) -> Result<(), T> {
        self.push_back(value);
        Ok(())
    }
    #[inline(always)]
    fn pop(&mut self) -> Option<T> {
        self.pop_front()
    }
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    #[inline(always)]
    fn retire(&mut self) -> Option<Self> {
        (self.is_empty() && self.capacity() > 1024).then(|| core::mem::take(self))
    }
}

#[doc(hidden)]
pub struct State<Q: Buffer> {
    pending: Option<Q>,
    senders: usize,
    waker: Option<Waker>,
}
impl<Q: Buffer> State<Q> {
    const fn new(queue: Q) -> Self {
        Self {
            pending: Some(queue),
            senders: 1,
            waker: None,
        }
    }
}

#[doc(hidden)]
pub trait Owner: sealed::Owner + Clone {
    type Queue: Buffer;
    fn state(&self) -> &RefCell<State<Self::Queue>>;
}
impl<Q: Buffer> sealed::Owner for &RefCell<State<Q>> {}
impl<Q: Buffer> Owner for &RefCell<State<Q>> {
    type Queue = Q;
    #[inline(always)]
    fn state(&self) -> &RefCell<State<Q>> {
        self
    }
}
#[cfg(feature = "alloc")]
impl<Q: Buffer> sealed::Owner for alloc::rc::Rc<RefCell<State<Q>>> {}
#[cfg(feature = "alloc")]
impl<Q: Buffer> Owner for alloc::rc::Rc<RefCell<State<Q>>> {
    type Queue = Q;
    #[inline(always)]
    fn state(&self) -> &RefCell<State<Q>> {
        self
    }
}

pub struct Sender<O: Owner>(O);
pub struct Receiver<O: Owner>(O);
pub type Borrowed<'a, T, const N: usize> = &'a RefCell<State<Ring<T, N>>>;
pub type BoundedSender<'a, T, const N: usize> = Sender<Borrowed<'a, T, N>>;
pub type BoundedReceiver<'a, T, const N: usize> = Receiver<Borrowed<'a, T, N>>;
impl<O: Owner> Sender<O> {
    #[inline]
    pub fn send(
        &self,
        value: <O::Queue as Buffer>::Item,
    ) -> Result<(), <O::Queue as Buffer>::Item> {
        let wake = {
            let mut state = self.0.state().borrow_mut();
            let Some(pending) = &mut state.pending else {
                return Err(value);
            };
            pending.push(value)?;
            state.waker.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }
}
impl<O: Owner> Clone for Sender<O> {
    fn clone(&self) -> Self {
        let mut state = self.0.state().borrow_mut();
        state.senders = state
            .senders
            .checked_add(1)
            .expect("admission sender overflow");
        drop(state);
        Self(self.0.clone())
    }
}
impl<O: Owner> Drop for Sender<O> {
    fn drop(&mut self) {
        let wake = {
            let mut state = self.0.state().borrow_mut();
            state.senders -= 1;
            if state.senders == 0 {
                state.waker.take()
            } else {
                None
            }
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
impl<O: Owner> Receiver<O> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0
            .state()
            .borrow()
            .pending
            .as_ref()
            .is_none_or(Buffer::is_empty)
    }
    #[inline]
    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<<O::Queue as Buffer>::Item>> {
        let same_waker = {
            let state = self.0.state().borrow();
            let same = state
                .waker
                .as_ref()
                .is_some_and(|wake| wake.will_wake(cx.waker()));
            if same && state.senders != 0 && state.pending.as_ref().is_some_and(Buffer::is_empty) {
                return Poll::Pending;
            }
            same
        };
        // Custom waker cloning and destruction may reenter this channel.
        let mut replacement = (!same_waker).then(|| cx.waker().clone());
        let (result, retired) = {
            let mut state = self.0.state().borrow_mut();
            let pending = state.pending.as_mut().expect("live receiver");
            let next = pending.pop();
            let retired = pending.retire();
            let result = if next.is_some() || state.senders == 0 {
                Poll::Ready(next)
            } else {
                if replacement.is_some() {
                    core::mem::swap(&mut replacement, &mut state.waker);
                }
                Poll::Pending
            };
            (result, retired)
        };
        drop(replacement);
        drop(retired);
        result
    }
}
impl<O: Owner> Drop for Receiver<O> {
    fn drop(&mut self) {
        let (pending, waker) = {
            let mut state = self.0.state().borrow_mut();
            (state.pending.take(), state.waker.take())
        };
        drop(waker);
        drop(pending);
    }
}

/// A reusable caller-owned admission channel. Endpoints borrow the slot, so
/// it cannot be reset or dropped while any accessible endpoint remains live.
/// The buffer and sender bookkeeping allocate nothing. Supplied wakers retain
/// their caller-selected ownership policy.
pub struct Slot<T, const N: usize> {
    state: RefCell<State<Ring<T, N>>>,
}
impl<T, const N: usize> Slot<T, N> {
    pub const fn new() -> Self {
        Self {
            state: RefCell::new(State::new(Ring::new())),
        }
    }
    pub fn pair(&mut self) -> (BoundedSender<'_, T, N>, BoundedReceiver<'_, T, N>) {
        *self.state.get_mut() = State::new(Ring::new());
        (Sender(&self.state), Receiver(&self.state))
    }
}
impl<T, const N: usize> Default for Slot<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "alloc")]
pub type Shared<T> = alloc::rc::Rc<RefCell<State<alloc::collections::VecDeque<T>>>>;
#[cfg(feature = "alloc")]
pub fn shared<T>() -> (Sender<Shared<T>>, Receiver<Shared<T>>) {
    let owner = alloc::rc::Rc::new(RefCell::new(
        State::new(alloc::collections::VecDeque::new()),
    ));
    (Sender(owner.clone()), Receiver(owner))
}

#[cfg(test)]
mod tests;
