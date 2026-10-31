//! Hosted streaming credit. Publication and acknowledgement own different
//! interests: credit can complete before Return without canceling that Return.
use super::*;

pub(crate) type Factory = dyn Fn() -> capnp::Result<Box<dyn crate::FlowController>>;

pub(super) struct Flow {
    factory: Rc<Factory>,
    controller: RefCell<Option<Box<dyn crate::FlowController>>>,
}
impl Flow {
    pub(super) fn new(factory: Rc<Factory>) -> Rc<Self> {
        Rc::new(Self {
            factory,
            controller: RefCell::new(None),
        })
    }
    fn prepare(&self) -> capnp::Result<()> {
        let mut controller = self.controller.borrow_mut();
        if controller.is_none() {
            *controller = Some((self.factory)()?);
        }
        Ok(())
    }
    fn sent(&self, words: usize, ack: Promise<(), Error>) -> Promise<(), Error> {
        let mut controller = self.controller.borrow_mut();
        if controller.is_none() {
            match (self.factory)() {
                Ok(created) => *controller = Some(created),
                Err(error) => return Promise::err(error),
            }
        }
        controller.as_mut().unwrap().sent(words, ack)
    }
}

/// Exact hosted streaming publication. A resource-aware output adapter retains
/// this owner with its frame and calls `published` once the transport accepts it.
/// A pending or rejected reservation must not grant byte credit.
pub struct Publication {
    flow: RefCell<Option<Rc<Flow>>>,
    ack: RefCell<Option<Promise<(), Error>>>,
    grant: RefCell<Option<Promise<(), Error>>>,
    words: Cell<Option<usize>>,
    wake: Cell<Option<Waker>>,
}
impl Publication {
    pub(super) fn new(flow: Rc<Flow>) -> Rc<Self> {
        Rc::new(Self {
            flow: RefCell::new(Some(flow)),
            ack: RefCell::new(None),
            grant: RefCell::new(None),
            words: Cell::new(None),
            wake: Cell::new(None),
        })
    }
    /// Initialize the connection's controller before handing this frame to
    /// its transport, as the ordinary message sender does.
    pub fn prepare(&self) -> capnp::Result<()> {
        let flow = self.flow.borrow().clone();
        flow.map_or(Ok(()), |flow| flow.prepare())
    }
    pub(super) fn bind(self: &Rc<Self>, ack: Promise<(), Error>) -> Promise<(), Error> {
        *self.ack.borrow_mut() = Some(ack);
        self.charge();
        let this = self.clone();
        Promise::from_future(std::future::poll_fn(move |cx| this.poll(cx)))
    }
    /// `words` excludes the framing segment table, as OutgoingMessage does.
    /// Repeated observations of this same publication do not charge it twice.
    pub fn published(&self, words: usize) {
        if self.words.get().is_some() {
            return;
        }
        self.words.set(Some(words));
        self.charge();
    }
    fn charge(&self) {
        let Some(words) = self.words.get() else {
            return;
        };
        let Some(ack) = self.ack.borrow_mut().take() else {
            return;
        };
        let flow = self
            .flow
            .borrow_mut()
            .take()
            .expect("one stream publication");
        let grant = flow.sent(words, ack);
        // Only the capability owns its controller after publication. A pending
        // credit promise must not prevent last-client Drop from releasing it.
        drop(flow);
        *self.grant.borrow_mut() = Some(grant);
        if let Some(wake) = self.wake.take() {
            wake.wake();
        }
    }
    fn poll(&self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        let previous = self.wake.replace(Some(cx.waker().clone()));
        drop(previous);
        // A local route or admission failure may complete without publishing a
        // frame. Preserve that exact completion/error instead of waiting forever.
        let ack = self.ack.borrow_mut().take();
        if let Some(mut ack) = ack {
            match Pin::new(&mut ack).poll(cx) {
                Poll::Ready(result) if self.words.get().is_none() => return self.finish(result),
                Poll::Ready(result) => {
                    *self.ack.borrow_mut() = Some(Promise::from_future(std::future::ready(result)))
                }
                Poll::Pending => *self.ack.borrow_mut() = Some(ack),
            }
            self.charge();
        }
        let grant = self.grant.borrow_mut().take();
        match grant {
            Some(mut grant) => match Pin::new(&mut grant).poll(cx) {
                Poll::Ready(result) => self.finish(result),
                Poll::Pending => {
                    *self.grant.borrow_mut() = Some(grant);
                    Poll::Pending
                }
            },
            None => Poll::Pending,
        }
    }
    fn finish(&self, result: capnp::Result<()>) -> Poll<capnp::Result<()>> {
        let flow = self.flow.borrow_mut().take();
        drop(flow);
        drop(self.wake.take());
        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[test]
    fn publication_binds_in_either_order_and_keeps_ack_after_credit_is_dropped() {
        for publish_first in [false, true] {
            let drivers = Rc::new(RefCell::new(Vec::new()));
            let observed = drivers.clone();
            let factories = Rc::new(Cell::new(0));
            let calls = factories.clone();
            let flow = Flow::new(Rc::new(move || {
                calls.set(calls.get() + 1);
                let (controller, driver) = crate::new_fixed_window_flow_controller(0);
                observed.borrow_mut().push(driver);
                Ok(controller)
            }));
            let publication = Publication::new(flow.clone());
            let (sender, receiver) = futures::channel::oneshot::channel();
            let acknowledged = Rc::new(Cell::new(false));
            let completed = acknowledged.clone();
            let ack = Promise::from_future(async move {
                receiver.await.unwrap();
                completed.set(true);
                Ok(())
            });
            if publish_first {
                publication.prepare().unwrap();
                publication.published(17);
            }
            let mut credit = publication.bind(ack);
            if !publish_first {
                assert!((&mut credit).now_or_never().is_none());
                publication.prepare().unwrap();
                publication.published(17);
            }
            publication.published(17); // observing the same accepted frame is idempotent
            assert_eq!(factories.get(), 1);
            futures::executor::block_on(credit).unwrap();
            drop(publication);
            drop(flow); // only the acknowledgement driver now owns the pending RPC
            let mut driver = drivers.borrow_mut().pop().unwrap();
            assert!((&mut driver).now_or_never().is_none());
            assert!(!acknowledged.get());
            sender.send(()).unwrap();
            futures::executor::block_on(driver).unwrap();
            assert!(acknowledged.get());
        }
    }

    #[test]
    fn unpublished_completion_does_not_create_a_controller_or_wait_for_publication() {
        for failed in [false, true] {
            let flow = Flow::new(Rc::new(|| panic!("an unpublished call has no byte charge")));
            let publication = Publication::new(flow);
            let result = if failed {
                Err(Error::overloaded(
                    "request rejected before publication".into(),
                ))
            } else {
                Ok(()) // the route resolved to a local service
            };
            let credit = publication.bind(Promise::from(result.clone()));
            let actual = futures::executor::block_on(credit);
            assert_eq!(
                actual.map_err(|error| error.kind),
                result.map_err(|error| error.kind)
            );
        }
    }
}
