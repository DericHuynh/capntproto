//! A reply embedded in its existing operation owner. Unlike a channel, this
//! has no separately owned endpoints, allocation, task or lifetime bookkeeping.
//! The owner serializes the unique producer and retains the operation until
//! polling or cancellation finishes. Values and wakers leave cell borrows
//! before polling, waking or destruction can reenter the owner.
use core::{
    cell::{Ref, RefCell, RefMut},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// Inline completion storage. `T` may borrow caller-owned resources and need
/// not allocate. A hosted driver can retain its existing owned promise here.
pub struct Reply<T> {
    value: Option<T>,
    waker: Option<Waker>,
    observed: bool,
}
impl<T> Default for Reply<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Reply<T> {
    pub const fn new() -> Self {
        Self {
            value: None,
            waker: None,
            observed: false,
        }
    }
}

/// Project a reply from its existing owner without adding another cell or
/// reference count. Implementations must always project the same reply.
/// `RefCell<Reply<T>>` also supports stack- or arena-owned operations directly.
pub trait Owner<T> {
    fn reply(&self) -> Ref<'_, Reply<T>>;
    fn reply_mut(&self) -> RefMut<'_, Reply<T>>;
}
impl<T> Owner<T> for RefCell<Reply<T>> {
    #[inline]
    fn reply(&self) -> Ref<'_, Reply<T>> {
        self.borrow()
    }
    #[inline]
    fn reply_mut(&self) -> RefMut<'_, Reply<T>> {
        self.borrow_mut()
    }
}

/// Poll outside the owner's borrow. A redirected reply may suspend again.
/// The owner must not poll after completion or cancellation.
#[inline]
pub fn poll<T: Future + Unpin>(owner: &impl Owner<T>, cx: &mut Context<'_>) -> Poll<T::Output> {
    let value = {
        let mut reply = owner.reply_mut();
        debug_assert!(!reply.observed, "question reply polled after completion");
        reply.value.take()
    };
    if let Some(mut value) = value {
        match Pin::new(&mut value).poll(cx) {
            Poll::Ready(result) => {
                owner.reply_mut().observed = true;
                return Poll::Ready(result);
            }
            Poll::Pending => owner.reply_mut().value = Some(value),
        }
    } else {
        let registered = owner
            .reply()
            .waker
            .as_ref()
            .is_some_and(|wake| wake.will_wake(cx.waker()));
        if !registered {
            let wake = cx.waker().clone();
            let previous = owner.reply_mut().waker.replace(wake);
            drop(previous);
            // Custom waker cloning/destruction can synchronously deliver.
            if owner.reply().value.is_some() {
                cx.waker().wake_by_ref();
            }
        }
    }
    Poll::Pending
}

/// Publish once using the operation's producer authority, then wake outside
/// its borrow. Returns a late or replaced value intact for caller cleanup.
/// This primitive does not provide a second producer or authorize a Return.
#[inline]
pub fn deliver<T>(owner: &impl Owner<T>, value: T) -> Option<T> {
    let (previous, wake) = {
        let mut reply = owner.reply_mut();
        if reply.observed {
            return Some(value);
        }
        (reply.value.replace(value), reply.waker.take())
    };
    if let Some(wake) = wake {
        wake.wake();
    }
    previous
}

/// End receiver interest. Detach user values and wakers before destruction.
#[inline]
pub fn release<T>(owner: &impl Owner<T>) {
    let (value, wake) = {
        let mut reply = owner.reply_mut();
        reply.observed = true;
        (reply.value.take(), reply.waker.take())
    };
    drop(value);
    drop(wake);
}

#[cfg(test)]
mod tests;
