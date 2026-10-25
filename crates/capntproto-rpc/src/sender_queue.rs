// Copyright (c) 2013-2016 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
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

use capntproto_rpc_engine::{completion::shared as oneshot, pending};
use futures::{FutureExt, TryFutureExt};

use alloc::rc::{Rc, Weak};
use core::cell::RefCell;

use capnp::capability::Promise;
use capnp::Error;

use crate::collections::BTreeMap;

type Inner<In, Out> = pending::State<In, Sender<Out>, BTreeMap<u64, (In, Sender<Out>)>>;

/// A queue representing tasks that consume input of type `In` and produce output of
/// type `Out`.
pub(crate) struct SenderQueue<In, Out>
where
    In: 'static,
    Out: 'static,
{
    inner: Option<Rc<RefCell<Inner<In, Out>>>>,
}

type Remover<In, Out> = pending::Remover<Weak<RefCell<Inner<In, Out>>>>;

impl<In, Out> SenderQueue<In, Out>
where
    In: 'static,
    Out: 'static,
{
    pub(crate) fn new() -> Self {
        Self { inner: None }
    }

    fn inner(&mut self) -> &Rc<RefCell<Inner<In, Out>>> {
        self.inner.get_or_insert_with(|| {
            Rc::new(RefCell::new(Inner {
                next_id: Some(0),
                first: None,
                map: BTreeMap::new(),
            }))
        })
    }

    /// Pushes `value` to the queue, returning a promise that resolves after
    /// `value` is consumed on the other end of the queue. If the returned promised
    /// is dropped, then `value` is removed from the queue.
    pub(crate) fn push(&mut self, value: In) -> Promise<Out, Error> {
        Promise::from_future(self.push_future(value))
    }

    /// Keep the queue receiver inside an existing continuation. The public
    /// hook still returns a boxed Promise, but forwarding needs no second box.
    pub(crate) fn push_future(
        &mut self,
        value: In,
    ) -> impl core::future::Future<Output = Result<Out, Error>> + Unpin + 'static {
        let (id, rx) = match self.enqueue(value, true) {
            Ok(admitted) => admitted,
            Err(value) => {
                drop(value);
                return futures::future::Either::Left(futures::future::ready(Err(
                    Error::overloaded("sender queue identities exhausted".into()),
                )));
            }
        };
        let remover = Remover::new(
            id,
            Rc::downgrade(self.inner.as_ref().expect("admitted queue")),
        );

        futures::future::Either::Right(
            rx.expect("observed queue entry")
                .map_err(|_| Error::failed("SenderQueue canceled".into()))
                .map(move |out| {
                    drop(remover);
                    out
                }),
        )
    }

    /// Detached work still needs a unique identity. Rejection returns its owner.
    pub(crate) fn push_detach(&mut self, value: In) -> Result<(), In> {
        self.enqueue(value, false).map(|_| ())
    }

    fn enqueue(
        &mut self,
        value: In,
        observed: bool,
    ) -> Result<(u64, Option<oneshot::Receiver<Out>>), In> {
        let mut inner = self.inner().borrow_mut();
        if inner.next_id.is_none() {
            return Err(value);
        }
        // Detached pipeline publication has no receiver. Preserve the same
        // rejected-value behavior without allocating a canceled channel.
        let (sender, receiver) = if observed {
            let (sender, receiver) = oneshot::channel();
            (Sender(Some(sender)), Some(receiver))
        } else {
            (Sender(None), None)
        };
        let id = inner
            .try_insert(value, sender)
            .map_err(|(value, _)| value)?;
        Ok((id, receiver))
    }

    pub(crate) fn drain(&mut self) -> Drain<In, Out> {
        // Old promises keep Remover handles after a drain. Never recycle their
        // identities: canceling one must not remove newly enqueued work.
        let (first, map) = match &self.inner {
            Some(inner) => {
                let mut inner = inner.borrow_mut();
                inner.take()
            }
            None => (None, BTreeMap::new()),
        };
        Drain::new(first, map.into_iter())
    }
}

pub(crate) type Drain<In, Out> = pending::Drain<
    In,
    Sender<Out>,
    crate::collections::btree_map::IntoIter<u64, (In, Sender<Out>)>,
>;

pub(crate) struct Sender<T>(Option<oneshot::Sender<T>>);
impl<T> Sender<T> {
    pub(crate) fn send(self, value: T) -> Result<(), T> {
        match self.0 {
            Some(sender) => sender.send(value),
            None => Err(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn canceling_a_drained_promise_cannot_remove_new_work() {
        let mut queue = SenderQueue::<u32, u32>::new();
        let old = queue.push(17);
        let (_, sender) = queue.drain().next().unwrap();
        let mut new = queue.push(23);
        drop(old);
        assert_eq!(
            usize::from(queue.inner.as_ref().unwrap().borrow().first.is_some())
                + queue.inner.as_ref().unwrap().borrow().map.len(),
            1,
            "stale cancellation must preserve new work"
        );
        assert!(sender.send(19).is_err());
        let (input, sender) = queue.drain().next().unwrap();
        assert_eq!(input, 23);
        sender.send(29).unwrap();
        assert_eq!((&mut new).now_or_never().unwrap().unwrap(), 29);
    }

    #[test]
    fn cancellation_drops_inputs_outside_the_queue_borrow() {
        struct Reenter(Rc<dyn Fn()>);
        impl Drop for Reenter {
            fn drop(&mut self) {
                (self.0)()
            }
        }
        let mut queue = SenderQueue::<Reenter, ()>::new();
        let weak = Rc::downgrade(queue.inner());
        let inspected = Rc::new(Cell::new(false));
        let flag = inspected.clone();
        let pending = queue.push(Reenter(Rc::new(move || {
            let inner = weak.upgrade().unwrap();
            assert!(
                inner.try_borrow_mut().is_ok(),
                "queue input drop must not retain a state borrow"
            );
            flag.set(true);
        })));
        drop(pending);
        assert!(inspected.get());
        assert!(queue.inner.as_ref().unwrap().borrow().first.is_none());
        assert!(queue.inner.as_ref().unwrap().borrow().map.is_empty());
    }

    #[test]
    fn identity_exhaustion_preserves_queued_work_and_rejects_detached_owners() {
        let mut queue = SenderQueue::<u32, u32>::new();
        queue.inner().borrow_mut().next_id = Some(u64::MAX);
        let mut last = queue.push(31);
        let failed = queue.push(37).now_or_never().unwrap().unwrap_err();
        assert_eq!(failed.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(queue.push_detach(41), Err(41));
        let (input, sender) = queue.drain().next().unwrap();
        assert_eq!(input, 31);
        sender.send(43).unwrap();
        assert_eq!((&mut last).now_or_never().unwrap().unwrap(), 43);
        assert_eq!(
            queue.push_detach(47),
            Err(47),
            "drain cannot reset identity exhaustion"
        );
    }
    #[test]
    fn mixed_entries_preserve_fifo_after_first_cancellation_and_detached_completion() {
        let mut queue = SenderQueue::<u32, u32>::new();
        assert!(queue.drain().next().is_none());
        assert!(queue.inner.is_none());
        let first = queue.push(1);
        assert_eq!(queue.push_detach(2), Ok(()));
        let third = queue.push(3);
        drop(first);
        // Reusing the vacant inline slot must not overtake older tree entries.
        let fourth = queue.push(4);
        let mut drained = queue.drain();
        let (value, detached) = drained.next().unwrap();
        assert_eq!(value, 2);
        assert_eq!(detached.send(20), Err(20));
        let (value, sender) = drained.next().unwrap();
        assert_eq!(value, 3);
        sender.send(30).unwrap();
        let (value, sender) = drained.next().unwrap();
        assert_eq!(value, 4);
        sender.send(40).unwrap();
        assert!(drained.next().is_none());
        assert_eq!(third.now_or_never().unwrap().unwrap(), 30);
        assert_eq!(fourth.now_or_never().unwrap().unwrap(), 40);
        // Detached completion returns its owner without calling its destructor.
        let dropped = Rc::new(Cell::new(0));
        struct Dropped(Rc<Cell<usize>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let mut queue = SenderQueue::<(), Dropped>::new();
        assert!(queue.push_detach(()).is_ok());
        let (_, sender) = queue.drain().next().unwrap();
        let value = sender.send(Dropped(dropped.clone())).err().unwrap();
        assert_eq!(dropped.get(), 0);
        drop(value);
        assert_eq!(dropped.get(), 1);
    }
}
