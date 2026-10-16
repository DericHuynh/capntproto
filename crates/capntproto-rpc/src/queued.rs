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

use capnp::any_pointer;
use capnp::capability::CallHints;
use capnp::capability::Promise;
use capnp::private::capability::{ClientHook, ParamsHook, PipelineHook, PipelineOp, ResultsHook};
use capnp::Error;

use futures::{Future, FutureExt, TryFutureExt};

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::attach::Attach;
use crate::sender_queue::SenderQueue;
use crate::{broken, local};

// A published pipeline can resolve a queued client before the call which drives
// it completes. Stop driving that call once forwarding/resolution is delivered:
// its later failure must not replace the independently published capability.
fn drive_until_ready<T: 'static>(
    promise: Promise<T, Error>,
    driver: futures::future::Shared<Promise<(), Error>>,
) -> Promise<T, Error> {
    Promise::from_future(async move {
        match futures::future::select(promise, driver).await {
            futures::future::Either::Left((result, _)) => result,
            futures::future::Either::Right((Ok(()), promise)) => promise.await,
            futures::future::Either::Right((Err(error), mut promise)) => {
                // Polling the driver may itself have published the pipeline
                // before failing. Give that publication precedence as well.
                (&mut promise).now_or_never().unwrap_or(Err(error))
            }
        }
    })
}

pub(crate) struct PipelineInner {
    // Once the promise resolves, this will become non-null and point to the underlying object.
    redirect: Option<Box<dyn PipelineHook>>,

    promise_to_drive: Option<futures::future::Shared<Promise<(), Error>>>,

    clients_to_resolve: SenderQueue<(Weak<RefCell<ClientInner>>, Vec<PipelineOp>), ()>,
}

impl PipelineInner {
    fn resolve(this: &Rc<RefCell<Self>>, result: Result<Box<dyn PipelineHook>, Error>) {
        if this.borrow().redirect.is_some() {
            // Already resolved, probably by set_pipeline().
            return;
        }

        let pipeline = match result {
            Ok(pipeline_hook) => pipeline_hook,
            Err(e) => Box::new(broken::Pipeline::new(e)),
        };

        this.borrow_mut().redirect = Some(pipeline.add_ref());

        let pending = this.borrow_mut().clients_to_resolve.drain();
        for ((weak_client, ops), waiter) in pending {
            if let Some(client) = weak_client.upgrade() {
                let clienthook = pipeline.get_pipelined_cap_move(ops);
                ClientInner::resolve(&client, Ok(clienthook));
            }
            let _ = waiter.send(());
        }

        // Dropping the last driver fork can destroy its PipelineInnerSender,
        // which resolves this pipeline again. Publish the redirect and detach
        // the old driver before running that destructor.
        let old_driver = this.borrow_mut().promise_to_drive.take();
        drop(old_driver);
    }
}

pub(crate) struct PipelineInnerSender {
    inner: Option<Weak<RefCell<PipelineInner>>>,
    resolve_on_drop: bool,
}

impl PipelineInnerSender {
    // Preserve the single-use set_pipeline contract even when the caller
    // explicitly promises not to pipeline on this answer.
    pub(crate) fn disabled() -> Self {
        Self {
            inner: None,
            resolve_on_drop: false,
        }
    }

    pub(crate) fn weak_clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            resolve_on_drop: false,
        }
    }
}

impl Drop for PipelineInnerSender {
    fn drop(&mut self) {
        if self.resolve_on_drop {
            if let Some(weak_queued) = self.inner.take() {
                if let Some(pipeline_inner) = weak_queued.upgrade() {
                    PipelineInner::resolve(
                        &pipeline_inner,
                        Ok(Box::new(crate::broken::Pipeline::new(Error::failed(
                            "PipelineInnerSender was canceled".into(),
                        )))),
                    );
                }
            }
        }
    }
}

impl PipelineInnerSender {
    pub(crate) fn complete(self, pipeline: Box<dyn PipelineHook>) {
        self.complete_with(|| pipeline);
    }

    pub(crate) fn complete_with(mut self, pipeline: impl FnOnce() -> Box<dyn PipelineHook>) {
        if let Some(weak_queued) = self.inner.take() {
            if let Some(pipeline_inner) = weak_queued.upgrade() {
                crate::queued::PipelineInner::resolve(&pipeline_inner, Ok(pipeline()));
            }
        }
    }
}

pub(crate) struct Pipeline {
    inner: Rc<RefCell<PipelineInner>>,
}

impl Pipeline {
    pub(crate) fn new() -> (PipelineInnerSender, Self) {
        let inner = Rc::new(RefCell::new(PipelineInner {
            redirect: None,
            promise_to_drive: None,
            clients_to_resolve: SenderQueue::new(),
        }));

        (
            PipelineInnerSender {
                inner: Some(Rc::downgrade(&inner)),
                resolve_on_drop: true,
            },
            Self { inner },
        )
    }

    pub(crate) fn drive<F>(&mut self, promise: F)
    where
        F: Future<Output = Result<(), Error>> + 'static + Unpin,
    {
        let previous = self.inner.borrow().promise_to_drive.clone();
        let new = match previous {
            Some(previous) => {
                Promise::from_future(futures::future::try_join(previous, promise).map_ok(|_| ()))
            }
            None => Promise::from_future(promise),
        }
        .shared();
        let old_driver = self.inner.borrow_mut().promise_to_drive.replace(new);
        drop(old_driver);
    }
}

impl Clone for Pipeline {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl PipelineHook for Pipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(self.clone())
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        self.get_pipelined_cap_move(ops.into())
    }

    fn get_pipelined_cap_move(&self, ops: Vec<PipelineOp>) -> Box<dyn ClientHook> {
        if let Some(p) = &self.inner.borrow().redirect {
            return p.get_pipelined_cap_move(ops);
        }

        let mut queued_client = Client::new(Some(self.inner.clone()));
        if let Some(driver) = self.inner.borrow().promise_to_drive.clone() {
            queued_client.drive(driver);
        }
        let weak_queued = Rc::downgrade(&queued_client.inner);
        let admitted = self
            .inner
            .borrow_mut()
            .clients_to_resolve
            .push_detach((weak_queued, ops));
        if admitted.is_err() {
            return broken::new_cap(Error::overloaded(
                "pipeline queue identities exhausted".into(),
            ));
        }

        Box::new(queued_client)
    }
}

pub(crate) struct ClientInner {
    // Once the promise resolves, this will become non-null and point to the underlying object.
    // A resolved target is immutable. Clone its owner without allocating a new
    // hook or retaining a cell borrow across application callbacks.
    redirect: Option<Rc<dyn ClientHook>>,

    // The queued::PipelineInner that this client is derived from, if any. We need to hold on
    // to a reference to it so that it doesn't get canceled before the client is resolved.
    pipeline_inner: Option<Rc<RefCell<PipelineInner>>>,

    promise_to_drive: Option<futures::future::Shared<Promise<(), Error>>>,

    // When this promise resolves, each queued call will be forwarded to the real client.  This needs
    // to occur *before* any 'whenMoreResolved()' promises resolve, because we want to make sure
    // previously-queued calls are delivered before any new calls made in response to the resolution.
    call_forwarding_queue: SenderQueue<
        (
            u64,
            u16,
            Box<dyn ParamsHook>,
            Box<dyn ResultsHook>,
            CallHints,
        ),
        Promise<(), Error>,
    >,

    // whenMoreResolved() returns forks of this promise.  These must resolve *after* queued calls
    // have been initiated (so that any calls made in the whenMoreResolved() handler are correctly
    // delivered after calls made earlier), but *before* any queued calls return (because it might
    // confuse the application if a queued call returns before the capability on which it was made
    // resolves).  Luckily, we know that queued calls will involve, at the very least, an
    // eventLoop.evalLater.
    client_resolution_queue: SenderQueue<(), Box<dyn ClientHook>>,
}

impl ClientInner {
    pub(crate) fn resolve(state: &Rc<RefCell<Self>>, result: Result<Box<dyn ClientHook>, Error>) {
        assert!(state.borrow().redirect.is_none());
        let client: Rc<dyn ClientHook> = Rc::from(match result {
            Ok(clienthook) => clienthook,
            Err(e) => broken::new_cap(e),
        });
        state.borrow_mut().redirect = Some(client.clone());
        let pending = state.borrow_mut().call_forwarding_queue.drain();
        for (args, waiter) in pending {
            let (interface_id, method_id, params, results, hints) = args;
            let result_promise =
                client.call_with_hints(interface_id, method_id, params, results, hints);
            let _ = waiter.send(result_promise);
        }

        let pending = state.borrow_mut().client_resolution_queue.drain();
        for ((), waiter) in pending {
            let _ = waiter.send(client.add_ref());
        }
        let (driver, pipeline) = {
            let mut state = state.borrow_mut();
            (state.promise_to_drive.take(), state.pipeline_inner.take())
        };
        drop((driver, pipeline));
    }
}

pub(crate) struct Client {
    pub(crate) inner: Rc<RefCell<ClientInner>>,
}

impl Client {
    pub(crate) fn new(pipeline_inner: Option<Rc<RefCell<PipelineInner>>>) -> Self {
        let inner = Rc::new(RefCell::new(ClientInner {
            promise_to_drive: None,
            pipeline_inner,
            redirect: None,
            call_forwarding_queue: SenderQueue::new(),
            client_resolution_queue: SenderQueue::new(),
        }));
        Self { inner }
    }

    pub(crate) fn drive<F>(&mut self, promise: F)
    where
        F: Future<Output = Result<(), Error>> + 'static,
    {
        assert!(self.inner.borrow().promise_to_drive.is_none());
        self.inner.borrow_mut().promise_to_drive = Some(Promise::from_future(promise).shared());
    }
}

impl ClientHook for Client {
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        let Ok(state) = self.inner.try_borrow() else {
            chain.push("promise(busy)");
            return;
        };
        let redirect = state.redirect.clone();
        drop(state);
        if let Some(redirect) = redirect {
            chain.push("resolved");
            chain.follow(&*redirect);
        } else {
            chain.push("promise");
        }
    }

    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self {
            inner: self.inner.clone(),
        })
    }
    fn new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<::capnp::MessageSize>,
    ) -> ::capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        // Preserve the resolved destination's request-time admission. Invoke
        // hooks outside the cell borrow, since custom hooks may reenter.
        let redirect = self.inner.borrow().redirect.clone();
        if let Some(client) = redirect {
            return client.new_call(interface_id, method_id, size_hint);
        }
        ::capnp::capability::Request::new(Box::new(local::Request::new(
            interface_id,
            method_id,
            size_hint,
            self.add_ref(),
        )))
    }

    fn new_call_with_hints(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<capnp::MessageSize>,
        hints: CallHints,
    ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        let redirect = self.inner.borrow().redirect.clone();
        if let Some(client) = redirect {
            return client.new_call_with_hints(interface_id, method_id, size_hint, hints);
        }
        let mut request = self.new_call(interface_id, method_id, size_hint);
        request.hook.set_hints(hints);
        request
    }

    fn call(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.call_with_hints(
            interface_id,
            method_id,
            params,
            results,
            CallHints::default(),
        )
    }
    fn call_with_hints(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        let redirect = self.inner.borrow().redirect.clone();
        if let Some(client) = redirect {
            return client.call_with_hints(interface_id, method_id, params, results, hints);
        }

        let inner_clone = self.inner.clone();
        let forwarded = self
            .inner
            .borrow_mut()
            .call_forwarding_queue
            .push((interface_id, method_id, params, results, hints))
            .attach(inner_clone);

        // Drive until the call is forwarded, not until the forwarded call
        // finishes. The latter may depend on a different, still-pending promise.
        let forwarded = Promise::from_future(forwarded);
        let forwarded = match &self.inner.borrow().promise_to_drive {
            Some(driver) => drive_until_ready(forwarded, driver.clone()),
            None => forwarded,
        };
        Promise::from_future(forwarded.and_then(|call| call))
    }

    fn get_ptr(&self) -> usize {
        (&*self.inner.borrow()) as *const _ as usize
    }

    fn get_brand(&self) -> usize {
        0
    }

    #[cfg(unix)]
    fn get_fd(&self) -> Option<std::rc::Rc<std::os::fd::OwnedFd>> {
        let redirect = self.inner.borrow().redirect.clone();
        redirect.and_then(|inner| inner.get_fd())
    }

    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        let redirect = self.inner.borrow().redirect.clone();
        redirect.map(|inner| inner.add_ref())
    }

    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        let redirect = self.inner.borrow().redirect.clone();
        if let Some(client) = redirect {
            return Some(Promise::ok(client.add_ref()));
        }

        let promise = Promise::from_future(
            self.inner
                .borrow_mut()
                .client_resolution_queue
                .push(())
                .attach(self.inner.clone()),
        );
        match &self.inner.borrow().promise_to_drive {
            Some(p) => Some(drive_until_ready(promise, p.clone())),
            None => Some(promise),
        }
    }

    fn when_resolved(&self) -> Promise<(), Error> {
        crate::rpc::default_when_resolved_impl(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };
    thread_local! { static ON_WAKE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None); }
    struct Reenter;
    impl Wake for Reenter {
        fn wake(self: Arc<Self>) {
            let callback = ON_WAKE.with(|slot| slot.borrow_mut().take());
            if let Some(callback) = callback {
                callback()
            }
        }
    }
    #[test]
    fn optional_pipeline_drivers_keep_dependencies_and_reentrant_cleanup() {
        for failing in [0, 1] {
            let (_sender, mut pipeline) = Pipeline::new();
            for index in 0..2 {
                pipeline.drive(if index == failing {
                    Promise::err(Error::failed(format!("driver {index}")))
                } else {
                    Promise::ok(())
                });
            }
            let client = pipeline.get_pipelined_cap(&[]);
            let error = futures::executor::block_on(client.when_resolved()).unwrap_err();
            assert_eq!(error.extra, format!("driver {failing}"));
        }
        struct ReenterDrop(Weak<RefCell<PipelineInner>>, Rc<std::cell::Cell<bool>>);
        impl Drop for ReenterDrop {
            fn drop(&mut self) {
                let inner = self.0.upgrade().unwrap();
                {
                    let state = inner
                        .try_borrow_mut()
                        .expect("driver drops outside the pipeline borrow");
                    assert!(state.redirect.is_some());
                }
                PipelineInner::resolve(&inner, Err(Error::failed("late driver failure".into())));
                self.1.set(true);
            }
        }
        let (sender, mut pipeline) = Pipeline::new();
        let dropped = Rc::new(std::cell::Cell::new(false));
        let probe = ReenterDrop(Rc::downgrade(&pipeline.inner), dropped.clone());
        pipeline.drive(futures::future::pending::<Result<(), Error>>().attach(probe));
        sender.complete(Box::new(broken::Pipeline::new(Error::failed(
            "published".into(),
        ))));
        assert!(dropped.get());
        let target = pipeline.get_pipelined_cap(&[]);
        assert_eq!(
            futures::executor::block_on(target.when_resolved())
                .unwrap_err()
                .extra,
            "published"
        );

        // A pipeline with no driving future still forwards its waiting clients.
        let (sender, pipeline) = Pipeline::new();
        let client = pipeline.get_pipelined_cap(&[]);
        let mut resolved = client.when_resolved();
        assert!(Pin::new(&mut resolved)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        sender.complete(Box::new(broken::Pipeline::new(Error::failed(
            "no driver".into(),
        ))));
        assert_eq!(
            futures::executor::block_on(resolved).unwrap_err().extra,
            "no driver"
        );
    }

    #[test]
    fn resolution_wakes_after_releasing_the_queued_client() {
        let client = Client::new(None);
        let inner = client.inner.clone();
        let mut pending = client.when_more_resolved().unwrap();
        let observed = Rc::new(std::cell::Cell::new(false));
        ON_WAKE.with(|slot| {
            let inner = inner.clone();
            let observed = observed.clone();
            *slot.borrow_mut() = Some(Box::new(move || {
                let state = inner
                    .try_borrow_mut()
                    .expect("resolution wakes outside queued client borrow");
                assert!(state.redirect.is_some());
                observed.set(true);
            }));
        });
        let waker = Waker::from(Arc::new(Reenter));
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut pending).poll(&mut cx).is_pending());
        ClientInner::resolve(&inner, Err(Error::failed("test rejection".into())));
        assert!(observed.get());
        assert!(matches!(
            Pin::new(&mut pending).poll(&mut cx),
            Poll::Ready(Ok(_))
        ));
    }
    #[test]
    fn resolved_requests_share_the_target_and_release_the_cell_before_callbacks() {
        use std::cell::Cell;
        struct Target {
            state: Weak<RefCell<ClientInner>>,
            refs: Rc<Cell<usize>>,
            calls: Rc<Cell<usize>>,
        }
        impl Target {
            fn reenter(&self) {
                let state = self.state.upgrade().unwrap();
                assert!(
                    state.try_borrow_mut().is_ok(),
                    "callback held the queued cell"
                );
            }
        }
        impl ClientHook for Target {
            fn add_ref(&self) -> Box<dyn ClientHook> {
                self.reenter();
                self.refs.set(self.refs.get() + 1);
                broken::new_cap(Error::failed("test target".into()))
            }
            fn new_call(
                &self,
                interface: u64,
                method: u16,
                size: Option<capnp::MessageSize>,
            ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
                self.reenter();
                self.calls.set(self.calls.get() + 1);
                broken::new_cap(Error::failed("test request".into()))
                    .new_call(interface, method, size)
            }
            fn call(
                &self,
                _: u64,
                _: u16,
                _: Box<dyn ParamsHook>,
                _: Box<dyn ResultsHook>,
            ) -> Promise<(), Error> {
                self.reenter();
                Promise::ok(())
            }
            fn get_brand(&self) -> usize {
                0
            }
            fn get_ptr(&self) -> usize {
                self as *const Self as usize
            }
            fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
                None
            }
            fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
                None
            }
            fn when_resolved(&self) -> Promise<(), Error> {
                Promise::ok(())
            }
            #[cfg(unix)]
            fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
                self.reenter();
                None
            }
        }
        let client = Client::new(None);
        let refs = Rc::new(Cell::new(0));
        let calls = Rc::new(Cell::new(0));
        ClientInner::resolve(
            &client.inner,
            Ok(Box::new(Target {
                state: Rc::downgrade(&client.inner),
                refs: refs.clone(),
                calls: calls.clone(),
            })),
        );
        drop(client.new_call(1, 2, None));
        drop(client.new_call(3, 4, None));
        assert_eq!(calls.get(), 2);
        assert_eq!(refs.get(), 0, "forwarding allocated a temporary hook");
        assert!(client.get_resolved().is_some());
        assert!(client.when_more_resolved().is_some());
        assert_eq!(refs.get(), 2);
        #[cfg(unix)]
        assert!(client.get_fd().is_none());
    }
}
