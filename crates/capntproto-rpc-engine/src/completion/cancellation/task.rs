//! Task ownership over the same cancellation cells as hosted legacy answers.
use super::{Cancellation, Context, Future, Pin, Poll};

/// Keep the cancellation record alive for an interest or execution driver.
/// A caller-backed owner can borrow separately declared storage; a hosted owner
/// can retain its existing status allocation. Neither requires another wrapper
/// allocation or a static lifetime.
pub trait Owner {
    fn cancellation(&self) -> Cancellation<'_>;
}
impl<O: Owner + ?Sized> Owner for &O {
    #[inline(always)]
    fn cancellation(&self) -> Cancellation<'_> {
        (**self).cancellation()
    }
}

/// Dropping an admitted cancellation interest cancels its original task.
/// The connection must authorize cancellation before creating this guard; this
/// primitive does not select method policy or publish a protocol message.
pub struct Cancel<O: Owner>(O);
impl<O: Owner> Cancel<O> {
    #[inline(always)]
    pub fn new(owner: O) -> Self {
        Self(owner)
    }
}
impl<O: Owner> Drop for Cancel<O> {
    #[inline(always)]
    fn drop(&mut self) {
        self.0.cancellation().cancel();
    }
}

/// A driver for work that handles its own result/exception publication. As in
/// legacy answer execution, the inner result is discarded and the driver only
/// reports completion or cancellation. Cancelled work is not polled again.
///
/// Shutdown clears the parent registration before destroying the owner and
/// future, including an unpolled future. Keep that original flat field/drop
/// order: a hosted status may still be retained by the future's result guards.
/// Parameter, result and application storage retain their own allocation policy.
pub struct Task<O: Owner, F> {
    status: O,
    future: F,
}
impl<O: Owner, F> Task<O, F> {
    #[inline(always)]
    pub fn new(status: O, future: F) -> Self {
        Self { status, future }
    }
}
impl<O: Owner + Unpin, F: Future<Output = Result<(), E>> + Unpin, E> Future for Task<O, F> {
    type Output = Result<(), E>;
    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.status
            .cancellation()
            .poll_task(Pin::new(&mut this.future), cx)
            .map(Ok)
    }
}
impl<O: Owner, F> Drop for Task<O, F> {
    #[inline(always)]
    fn drop(&mut self) {
        self.status.cancellation().clear();
    }
}
