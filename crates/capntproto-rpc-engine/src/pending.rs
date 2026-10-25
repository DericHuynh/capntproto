//! Ordered pending calls, sharing legacy's inline entry and never-recycled
//! cancellation identities across borrowed and hosted storage. Owners detached
//! by cancellation or draining leave the table borrow before destruction.
use core::{cell::RefCell, ops::Deref};

mod sealed {
    pub trait Map {}
    pub trait Weak {}
}

pub trait Map<I, S>: sealed::Map + Default + IntoIterator<Item = (u64, (I, S))> {
    fn is_empty(&self) -> bool;
    fn try_insert(&mut self, id: u64, value: (I, S)) -> Result<(), (I, S)>;
    fn remove(&mut self, id: &u64) -> Option<(I, S)>;
}

/// Fixed overflow backing. The queue retains a separate inline first entry.
/// Capacity failure preserves both the input and its reply producer.
pub struct Fixed<I, S, const N: usize> {
    values: [Option<(u64, (I, S))>; N],
}
impl<I, S, const N: usize> Fixed<I, S, N> {
    pub const fn new() -> Self {
        Self {
            values: [const { None }; N],
        }
    }
}
impl<I, S, const N: usize> Default for Fixed<I, S, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<I, S, const N: usize> sealed::Map for Fixed<I, S, N> {}
impl<I, S, const N: usize> Map<I, S> for Fixed<I, S, N> {
    fn is_empty(&self) -> bool {
        self.values.iter().all(Option::is_none)
    }
    fn try_insert(&mut self, id: u64, value: (I, S)) -> Result<(), (I, S)> {
        let Some(slot) = self.values.iter_mut().find(|slot| slot.is_none()) else {
            return Err(value);
        };
        *slot = Some((id, value));
        Ok(())
    }
    fn remove(&mut self, id: &u64) -> Option<(I, S)> {
        self.values
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|v| v.0 == *id))?
            .take()
            .map(|(_, value)| value)
    }
}
pub struct FixedIter<I, S, const N: usize> {
    values: core::array::IntoIter<Option<(u64, (I, S))>, N>,
}
impl<I, S, const N: usize> Iterator for FixedIter<I, S, N> {
    type Item = (u64, (I, S));
    fn next(&mut self) -> Option<Self::Item> {
        self.values.find_map(core::convert::identity)
    }
}
impl<I, S, const N: usize> IntoIterator for Fixed<I, S, N> {
    type Item = (u64, (I, S));
    type IntoIter = FixedIter<I, S, N>;
    fn into_iter(mut self) -> Self::IntoIter {
        self.values
            .sort_unstable_by_key(|v| v.as_ref().map(|v| v.0));
        FixedIter {
            values: self.values.into_iter(),
        }
    }
}
#[cfg(feature = "alloc")]
impl<I, S> sealed::Map for alloc::collections::BTreeMap<u64, (I, S)> {}
#[cfg(feature = "alloc")]
impl<I, S> Map<I, S> for alloc::collections::BTreeMap<u64, (I, S)> {
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    #[inline(always)]
    fn try_insert(&mut self, id: u64, value: (I, S)) -> Result<(), (I, S)> {
        self.insert(id, value);
        Ok(())
    }
    #[inline(always)]
    fn remove(&mut self, id: &u64) -> Option<(I, S)> {
        self.remove(id)
    }
}

/// Low-level owner state. Hosted adapters keep the same three field types and
/// retain their own Rc/Weak allocation policy. Reset only with exclusive access
/// after all cancellation identities have ended.
#[doc(hidden)]
pub struct State<I, S, M> {
    pub next_id: Option<u64>,
    pub first: Option<(u64, I, S)>,
    pub map: M,
}
impl<I, S, M: Map<I, S>> State<I, S, M> {
    pub const fn new(map: M) -> Self {
        Self {
            next_id: Some(0),
            first: None,
            map,
        }
    }
    #[inline]
    pub fn try_insert(&mut self, value: I, sender: S) -> Result<u64, (I, S)> {
        let Some(id) = self.next_id else {
            return Err((value, sender));
        };
        if self.first.is_none() && self.map.is_empty() {
            self.first = Some((id, value, sender));
        } else {
            self.map.try_insert(id, (value, sender))?;
        }
        self.next_id = id.checked_add(1);
        Ok(id)
    }
    #[inline]
    pub fn remove(&mut self, id: u64) -> Option<(I, S)> {
        if self.first.as_ref().is_some_and(|(key, _, _)| *key == id) {
            self.first.take().map(|(_, value, sender)| (value, sender))
        } else {
            self.map.remove(&id)
        }
    }
    #[inline]
    pub fn take(&mut self) -> (Option<(I, S)>, M) {
        (
            self.first.take().map(|(_, value, sender)| (value, sender)),
            core::mem::take(&mut self.map),
        )
    }
}

pub trait Weak: sealed::Weak {
    type Input;
    type Sender;
    type Map: Map<Self::Input, Self::Sender>;
    type Lease: Deref<Target = RefCell<State<Self::Input, Self::Sender, Self::Map>>>;
    fn upgrade(&self) -> Option<Self::Lease>;
}
impl<I, S, M: Map<I, S>> sealed::Weak for &RefCell<State<I, S, M>> {}
impl<I, S, M: Map<I, S>> Weak for &RefCell<State<I, S, M>> {
    type Input = I;
    type Sender = S;
    type Map = M;
    type Lease = Self;
    #[inline(always)]
    fn upgrade(&self) -> Option<Self> {
        Some(*self)
    }
}
#[cfg(feature = "alloc")]
impl<I, S, M: Map<I, S>> sealed::Weak for alloc::rc::Weak<RefCell<State<I, S, M>>> {}
#[cfg(feature = "alloc")]
impl<I, S, M: Map<I, S>> Weak for alloc::rc::Weak<RefCell<State<I, S, M>>> {
    type Input = I;
    type Sender = S;
    type Map = M;
    type Lease = alloc::rc::Rc<RefCell<State<I, S, M>>>;
    #[inline(always)]
    fn upgrade(&self) -> Option<Self::Lease> {
        self.upgrade()
    }
}
pub struct Remover<O: Weak> {
    id: u64,
    owner: O,
}
impl<O: Weak> Remover<O> {
    pub fn new(id: u64, owner: O) -> Self {
        Self { id, owner }
    }
    pub fn identity(&self) -> u64 {
        self.id
    }
}
impl<O: Weak> Drop for Remover<O> {
    fn drop(&mut self) {
        if let Some(inner) = self.owner.upgrade() {
            let removed = inner.borrow_mut().remove(self.id);
            drop(removed);
        }
    }
}
pub struct Drain<I, S, T> {
    first: Option<(I, S)>,
    iter: T,
}
impl<I, S, T> Drain<I, S, T> {
    pub fn new(first: Option<(I, S)>, iter: T) -> Self {
        Self { first, iter }
    }
}
impl<I, S, T: Iterator<Item = (u64, (I, S))>> Iterator for Drain<I, S, T> {
    type Item = (I, S);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.first
            .take()
            .or_else(|| self.iter.next().map(|(_, value)| value))
    }
}

/// Caller-owned queue backing. Its exclusive borrow prevents reset while any
/// accessible borrowed queue or cancellation identity still refers to it.
pub struct Storage<I, S, M> {
    inner: RefCell<State<I, S, M>>,
}
impl<I, S, M: Map<I, S>> Storage<I, S, M> {
    pub const fn with_map(map: M) -> Self {
        Self {
            inner: RefCell::new(State::new(map)),
        }
    }
    pub fn borrow(&mut self) -> Queue<'_, I, S, M> {
        Queue { inner: &self.inner }
    }
}
pub struct Queue<'a, I, S, M> {
    inner: &'a RefCell<State<I, S, M>>,
}
impl<I, S, M> Clone for Queue<'_, I, S, M> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}
pub type BorrowedRemover<'a, I, S, M> = Remover<&'a RefCell<State<I, S, M>>>;
impl<'a, I, S, M: Map<I, S>> Queue<'a, I, S, M> {
    pub fn try_push(&self, value: I, sender: S) -> Result<BorrowedRemover<'a, I, S, M>, (I, S)> {
        let id = self.inner.borrow_mut().try_insert(value, sender)?;
        Ok(Remover::new(id, self.inner))
    }
    pub fn drain(&self) -> Drain<I, S, M::IntoIter> {
        let (first, map) = self.inner.borrow_mut().take();
        Drain::new(first, map.into_iter())
    }
}

/// Observe a caller-owned reply and retire its queue identity on completion or
/// cancellation. Neither a boxed continuation nor a shared allocation is needed.
pub struct Observed<R, C> {
    receiver: R,
    cancel: Option<C>,
}
impl<R, C> Observed<R, C> {
    pub fn new(receiver: R, cancel: C) -> Self {
        Self {
            receiver,
            cancel: Some(cancel),
        }
    }
}
impl<R: core::future::Future + Unpin, C: Unpin> core::future::Future for Observed<R, C> {
    type Output = R::Output;
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let this = self.get_mut();
        let result = core::pin::Pin::new(&mut this.receiver).poll(cx);
        if result.is_ready() {
            drop(this.cancel.take());
        }
        result
    }
}

#[cfg(test)]
mod tests;
