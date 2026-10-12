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

use capntproto_rpc_engine::completion::shared as oneshot;
use futures::{FutureExt, TryFutureExt};

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use capnp::capability::Promise;
use capnp::Error;

use std::collections::BTreeMap;

struct Inner<In, Out>
where
    In: 'static,
    Out: 'static,
{
    next_id: Option<u64>,
    map: BTreeMap<u64, (In, oneshot::Sender<Out>)>,
}

/// A queue representing tasks that consume input of type `In` and produce output of
/// type `Out`.
pub(crate) struct SenderQueue<In, Out>
where
    In: 'static,
    Out: 'static,
{
    inner: Rc<RefCell<Inner<In, Out>>>,
}

pub(crate) struct Remover<In, Out>
where
    In: 'static,
    Out: 'static,
{
    id: u64,
    inner: Weak<RefCell<Inner<In, Out>>>,
}

impl<In, Out> Drop for Remover<In, Out>
where
    In: 'static,
    Out: 'static,
{
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            let removed = inner.borrow_mut().map.remove(&self.id);
            drop(removed);
        }
    }
}

impl<In, Out> SenderQueue<In, Out>
where
    In: 'static,
    Out: 'static,
{
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                next_id: Some(0),
                map: BTreeMap::new(),
            })),
        }
    }

    /// Pushes `value` to the queue, returning a promise that resolves after
    /// `value` is consumed on the other end of the queue. If the returned promised
    /// is dropped, then `value` is removed from the queue.
    pub(crate) fn push(&mut self, value: In) -> Promise<Out, Error> {
        let (id, rx) = match self.enqueue(value) {
            Ok(admitted) => admitted,
            Err(value) => {
                drop(value);
                return Promise::err(Error::overloaded(
                    "sender queue identities exhausted".into(),
                ));
            }
        };
        let remover = Remover {
            id,
            inner: Rc::downgrade(&self.inner),
        };

        Promise::from_future(
            rx.map_err(|_| Error::failed("SenderQueue canceled".into()))
                .map(move |out| {
                    drop(remover);
                    out
                }),
        )
    }

    /// Detached work still needs a unique identity. Rejection returns its owner.
    pub(crate) fn push_detach(&mut self, value: In) -> Result<(), In> {
        self.enqueue(value).map(|(_, receiver)| drop(receiver))
    }

    fn enqueue(&mut self, value: In) -> Result<(u64, oneshot::Receiver<Out>), In> {
        let mut inner = self.inner.borrow_mut();
        let Some(id) = inner.next_id else {
            return Err(value);
        };
        let (sender, receiver) = oneshot::channel();
        inner.next_id = id.checked_add(1);
        inner.map.insert(id, (value, sender));
        Ok((id, receiver))
    }

    pub(crate) fn drain(&mut self) -> Drain<In, Out> {
        // Old promises keep Remover handles after a drain. Never recycle their
        // identities: canceling one must not remove newly enqueued work.
        let map = std::mem::take(&mut self.inner.borrow_mut().map);
        Drain {
            iter: map.into_iter(),
        }
    }
}

pub(crate) struct Drain<In, Out>
where
    In: 'static,
    Out: 'static,
{
    iter: ::std::collections::btree_map::IntoIter<u64, (In, oneshot::Sender<Out>)>,
}

impl<In, Out> ::std::iter::Iterator for Drain<In, Out>
where
    In: 'static,
    Out: 'static,
{
    type Item = (In, oneshot::Sender<Out>);
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(_k, v)| v)
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
            queue.inner.borrow().map.len(),
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
        let weak = Rc::downgrade(&queue.inner);
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
        assert!(queue.inner.borrow().map.is_empty());
    }

    #[test]
    fn identity_exhaustion_preserves_queued_work_and_rejects_detached_owners() {
        let mut queue = SenderQueue::<u32, u32>::new();
        queue.inner.borrow_mut().next_id = Some(u64::MAX);
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
}
