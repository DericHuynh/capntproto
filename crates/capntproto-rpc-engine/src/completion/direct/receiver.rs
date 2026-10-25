//! A reply receiver keeps its concrete question/slot owner, not a required Rc.
use super::Owner;
use core::{
    future::Future,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    task::{Context, Poll},
};

/// Unique observation of a reply in an existing owner. O may retain a hosted
/// reference count or borrow caller-owned context through a question lease.
/// Completed response values must retain their own frame/capability leases;
/// this receiver releases question interest as soon as polling completes.
///
/// ```compile_fail
/// use capntproto_rpc_engine::completion::direct::{Owner, Receiver};
/// fn cannot_escape<T: 'static, O: Owner<T> + 'static>(owner: &O) -> Receiver<&'static O, T> {
///     Receiver::new(owner)
/// }
/// ```
pub struct Receiver<O, T>
where
    O: Deref,
    O::Target: Owner<T>,
{
    reference: Option<O>,
    done: bool,
    marker: PhantomData<T>,
}
impl<O, T> Receiver<O, T>
where
    O: Deref,
    O::Target: Owner<T>,
{
    /// Transfer the operation's single observation authority. Cloning an owner
    /// for other protocol interest does not authorize a second receiver.
    pub fn new(owner: O) -> Self {
        Self {
            reference: Some(owner),
            done: false,
            marker: PhantomData,
        }
    }
    /// Transfer unique observation to another adapter without cancellation.
    pub fn into_owner(mut self) -> O {
        assert!(!self.done, "completed reply receiver");
        self.reference.take().expect("live reply")
    }
}
impl<O, T> Future for Receiver<O, T>
where
    O: Deref + Unpin,
    O::Target: Owner<T>,
    T: Future + Unpin,
{
    type Output = T::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        assert!(!this.done, "reply receiver polled after completion");
        let result = super::poll(this.reference.as_deref().expect("live reply"), cx);
        if result.is_ready() {
            this.done = true;
            drop(this.reference.take());
        }
        result
    }
}
impl<O, T> Drop for Receiver<O, T>
where
    O: Deref,
    O::Target: Owner<T>,
{
    fn drop(&mut self) {
        if let Some(owner) = self.reference.take() {
            super::release(&*owner);
        }
    }
}

#[cfg(test)]
mod tests;
