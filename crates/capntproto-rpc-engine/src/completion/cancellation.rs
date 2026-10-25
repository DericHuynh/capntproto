//! Cancellation interest borrowed from an existing operation record.
//! The view adds no stored owner, allocation, task or reference count. In
//! particular, the hosted adapter can retain the layout of its cached records.
use core::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// Single-threaded cancellation and task-wake registration in caller storage.
///
/// The caller owns the operation's lifetime. Before reusing the cells, end all
/// previous cancellation interests, clear the registration and reset the flag.
/// Waker callbacks and the supplied future retain their own allocation policy.
pub struct Cancellation<'a> {
    canceled: &'a Cell<bool>,
    waker: &'a RefCell<Option<Waker>>,
}

impl<'a> Cancellation<'a> {
    #[inline]
    pub const fn new(canceled: &'a Cell<bool>, waker: &'a RefCell<Option<Waker>>) -> Self {
        Self { canceled, waker }
    }

    /// Notify the registered task after publishing cancellation and ending the
    /// cell borrow. Repeated cancellation does not repeat a consumed wake.
    #[inline]
    pub fn cancel(&self) {
        self.canceled.set(true);
        let wake = self.waker.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    /// Detach registration during completion or driver teardown. Dropping a
    /// custom waker can reenter cancellation, so it runs outside the borrow.
    #[inline]
    pub fn clear(&self) {
        let wake = self.waker.borrow_mut().take();
        drop(wake);
    }

    /// Poll a task whose completion itself handles its output (for example by
    /// sending Return or Exception). The future's output is deliberately
    /// discarded; this reports only completion or cancellation to its driver.
    ///
    /// Cancellation does not poll the future. Its destruction remains the
    /// owner's responsibility, including when the driver is dropped unpolled.
    #[inline]
    pub fn poll_task<F: Future + ?Sized>(
        &self,
        future: Pin<&mut F>,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
        if self.canceled.get() {
            return Poll::Ready(());
        }
        let registered = self
            .waker
            .borrow()
            .as_ref()
            .is_some_and(|wake| wake.will_wake(cx.waker()));
        if !registered {
            let wake = cx.waker().clone();
            let previous = self.waker.borrow_mut().replace(wake);
            drop(previous);
        }
        // Custom clone/drop callbacks can synchronously cancel the task.
        if self.canceled.get() {
            return Poll::Ready(());
        }
        if future.poll(cx).is_ready() {
            self.clear();
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests;
