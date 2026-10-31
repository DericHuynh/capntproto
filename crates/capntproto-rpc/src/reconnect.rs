use capnp::capability::CallHints;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use capnp::capability::{FromClientHook, Promise};
use capnp::private::capability::{ClientHook, RequestHook};
use futures::TryFutureExt;

/// Trait implemented by the reconnecting client to set new connection out-of-band.
///
/// When using [`auto_reconnect`] or [`lazy_auto_reconnect`] it is not always optimal
/// to wait for a call to fail with [`Disconnected`](capnp::ErrorKind::Disconnected)
/// before replacing the client that is wrapped with a new fresh one.
///
/// Sometimes we know by other means that a client has gone away. It could be that we
/// have clients that automatically sends us a new capability when it reconnects to us.
///
/// For these situations you can use the implementation of this trait that you get from
/// [`auto_reconnect`] or [`lazy_auto_reconnect`] to manually set the target of the
/// wrapped client.
///
/// # Example
///
/// ```ignore
/// // The reconnecting client that automatically calls connect
/// let (foo_client, set_target) = auto_reconnect(|| {
///     Ok(new_future_client(connect()))
/// })?;
///
/// // do work with foo_client
/// ...
///
/// // We become aware that the client has gone so reconnect manually
/// set_target.set_target(new_future_client(connect()));
///
/// // do more work with foo_client
/// ...
/// ```
pub trait SetTarget<C> {
    /// Adds a new reference to this implementation of SetTarget.
    ///
    /// This is mostly to get around that `Clone` requires `Sized` and so you need this
    /// trick to get a copy of the `Box<dyn SetTarget<C>>` you got from making the
    /// reconnecting client.
    fn add_ref(&self) -> Box<dyn SetTarget<C>>;

    /// Sets the target client of the reconnecting client that this trait implementation is
    /// for.
    fn set_target(&self, target: C);

    /// Forget the current target. The next call connects lazily. In-flight
    /// requests keep their original target and cannot replace the new generation.
    fn reset(&self);
}

impl<C> Clone for Box<dyn SetTarget<C>> {
    fn clone(&self) -> Self {
        self.add_ref()
    }
}

struct ClientInner<C> {
    current: Option<Rc<dyn ClientHook>>,
    generation: Rc<()>,
    marker: PhantomData<C>,
}

struct Client<F, C> {
    inner: Rc<RefCell<ClientInner<C>>>,
    // User callbacks may replace/reset the target. Borrow only the callback
    // during invocation, never the connection state.
    connect: Rc<RefCell<F>>,
}

impl<F, C> Client<F, C>
where
    F: FnMut() -> capnp::Result<C>,
    F: 'static,
    C: FromClientHook,
    C: 'static,
{
    pub(crate) fn new(connect: F) -> Client<F, C> {
        Client {
            connect: Rc::new(RefCell::new(connect)),
            inner: Rc::new(RefCell::new(ClientInner {
                generation: Rc::new(()),
                current: None,
                marker: PhantomData,
            })),
        }
    }

    fn get_current(&self) -> (Rc<dyn ClientHook>, Rc<()>) {
        let generation = {
            let inner = self.inner.borrow();
            if let Some(current) = &inner.current {
                return (current.clone(), inner.generation.clone());
            }
            inner.generation.clone()
        };
        let Ok(mut connect) = self.connect.try_borrow_mut() else {
            return (
                Rc::from(crate::broken::new_cap(capnp::Error::overloaded(
                    "reentrant call while reconnecting".into(),
                ))),
                generation,
            );
        };
        let hook: Rc<dyn ClientHook> = Rc::from(match connect() {
            Ok(client) => client.into_client_hook(),
            Err(error) => crate::broken::new_cap(error),
        });
        drop(connect);
        let mut inner = self.inner.borrow_mut();
        if Rc::ptr_eq(&generation, &inner.generation) {
            inner.current = Some(hook.clone());
            (hook, generation)
        } else {
            // Snapshot route and generation together. No external hook runs
            // under the cell borrow, and a stale failure cannot reset a replacement.
            let current = inner.current.clone();
            let generation = inner.generation.clone();
            drop(inner);
            drop(hook);
            (
                current.unwrap_or_else(|| {
                    Rc::from(crate::broken::new_cap(capnp::Error::overloaded(
                        "target reset while reconnecting".into(),
                    )))
                }),
                generation,
            )
        }
    }

    fn wrap<T: 'static>(
        &self,
        generation: Rc<()>,
        promise: impl futures::Future<Output = Result<T, capnp::Error>> + 'static,
    ) -> Promise<T, capnp::Error> {
        let c = self.clone();
        Promise::from_future(promise.map_err(move |err| {
            if err.kind == capnp::ErrorKind::Disconnected
                && Rc::ptr_eq(&generation, &c.inner.borrow().generation)
            {
                let old = {
                    let mut inner = c.inner.borrow_mut();
                    inner.generation = Rc::new(());
                    inner.current.take()
                };
                // Destruction of the old capability can itself update the target.
                drop(old);
                drop(c.get_current());
            }
            err
        }))
    }
}

impl<F: 'static, C> SetTarget<C> for Client<F, C>
where
    F: 'static,
    C: FromClientHook,
    C: 'static,
{
    fn add_ref(&self) -> Box<dyn SetTarget<C>> {
        Box::new(self.clone())
    }

    fn set_target(&self, target: C) {
        let target = Rc::from(target.into_client_hook());
        let old = {
            let mut inner = self.inner.borrow_mut();
            inner.generation = Rc::new(());
            inner.current.replace(target)
        };
        drop(old);
    }

    fn reset(&self) {
        let old = {
            let mut inner = self.inner.borrow_mut();
            inner.generation = Rc::new(());
            inner.current.take()
        };
        drop(old);
    }
}

impl<F, C> Clone for Client<F, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            connect: self.connect.clone(),
        }
    }
}

impl<F, C> ClientHook for Client<F, C>
where
    F: FnMut() -> capnp::Result<C>,
    F: 'static,
    C: FromClientHook,
    C: 'static,
{
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        chain.push("reconnect");
        if self.connect.try_borrow().is_err() {
            chain.push("busy");
            return;
        }
        let Ok(state) = self.inner.try_borrow() else {
            chain.push("busy");
            return;
        };
        let current = state.current.clone();
        drop(state);
        if let Some(current) = current {
            chain.follow(&*current);
        } else {
            chain.push("disconnected");
        }
    }

    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(self.clone())
    }

    fn new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<capnp::MessageSize>,
    ) -> capnp::capability::Request<capnp::any_pointer::Owned, capnp::any_pointer::Owned> {
        let (current, generation) = self.get_current();
        let result = current.new_call(interface_id, method_id, size_hint);
        let hook = Request::new(self.clone(), generation, result.hook);
        capnp::capability::Request::new(Box::new(hook))
    }

    fn call(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn capnp::private::capability::ParamsHook>,
        results: Box<dyn capnp::private::capability::ResultsHook>,
    ) -> Promise<(), capnp::Error> {
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
        params: Box<dyn capnp::private::capability::ParamsHook>,
        results: Box<dyn capnp::private::capability::ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), capnp::Error> {
        let (current, generation) = self.get_current();
        let result = current.call_with_hints(interface_id, method_id, params, results, hints);
        self.wrap(generation, result)
    }

    fn get_brand(&self) -> usize {
        0
    }

    fn get_ptr(&self) -> usize {
        (self.inner.as_ref()) as *const _ as usize
    }

    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        None
    }

    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, capnp::Error>> {
        None
    }

    fn when_resolved(&self) -> Promise<(), capnp::Error> {
        Promise::ok(())
    }
}

struct Request<F, C> {
    parent: Client<F, C>,
    generation: Rc<()>,
    inner: Box<dyn RequestHook>,
}

impl<F, C> Request<F, C> {
    fn new(parent: Client<F, C>, generation: Rc<()>, inner: Box<dyn RequestHook>) -> Request<F, C> {
        Request {
            parent,
            generation,
            inner,
        }
    }
}

impl<F, C> RequestHook for Request<F, C>
where
    F: FnMut() -> capnp::Result<C>,
    F: 'static,
    C: FromClientHook,
    C: 'static,
{
    fn set_hints(&mut self, hints: CallHints) {
        self.inner.set_hints(hints);
    }
    fn send_for_pipeline(self: Box<Self>) -> capnp::any_pointer::Pipeline {
        self.inner.send_for_pipeline()
    }

    fn get(&mut self) -> capnp::any_pointer::Builder<'_> {
        self.inner.get()
    }

    fn get_brand(&self) -> usize {
        0
    }

    fn send(self: Box<Self>) -> capnp::capability::RemotePromise<capnp::any_pointer::Owned> {
        let parent = self.parent;
        let mut result = self.inner.send();
        result.promise =
            capnp::capability::ResponsePromise::new(parent.wrap(self.generation, result.promise));
        result
    }

    fn send_streaming(self: Box<Self>) -> Promise<(), capnp::Error> {
        self.parent
            .wrap(self.generation, self.inner.send_streaming())
    }

    fn tail_send(
        self: Box<Self>,
    ) -> Option<(
        u32,
        Promise<(), capnp::Error>,
        Box<dyn capnp::private::capability::PipelineHook>,
    )> {
        // The wrapper owns reconnection semantics and has no connection brand.
        // ResultsHook must use normal forwarding for this request.
        None
    }
}

/// Creates a new client that reconnects when getting [`ErrorKind::Disconnected`](capnp::ErrorKind::Disconnected) errors.
///
/// Usually when you get a [`Disconnected`](capnp::ErrorKind::Disconnected) error response from calling a method on a capability
/// it means the end of that capability for good. And so you can't call methods on that
/// capability any more.
///
/// When you have a way of getting the capability back: Be it from a bootstrap or because
/// the capability is persistent this method can help you wrap that reconnection logic into a client
/// that automatically runs the logic whenever a method call returns [`Disconnected`](capnp::ErrorKind::Disconnected).
///
/// The way it works is that you provide a closure that returns a fresh client or a permanent error and
/// you get a new connected client and a [`SetTarget`] interface that you can optionally use to prematurely
/// replace the client.
///
/// There is one caveat though: The original request that got a [`Disconnected`](capnp::ErrorKind::Disconnected)
/// will still get that response. It is up to the caller to retry the call if relevant. `auto_reconnect`` only
/// deals with the calls that come after.
///
/// # Example
///
/// ```capnp
/// # Cap'n Proto schema
/// interface Foo {
///     identity @0 (x: UInt32) -> (y: UInt32);
/// }
/// ```
///
/// ```ignore
/// // A simple bootstrapped tcp connection to remote.example.com
/// async fn connect() -> capnp::Result<foo_client::Client> {
///     let stream = tokio::net::TcpStream::connect(&"remote.example.com:3001").await?;
///     stream.set_nodelay(true)?;
///     let (reader, writer) = tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
///
///     let network = Box::new(twoparty::VatNetwork::new(
///         futures::io::BufReader::new(reader),
///         futures::io::BufWriter::new(writer),
///         rpc_twoparty_capnp::Side::Client,
///         Default::default(),
///     ));
///
///     let mut rpc_system = RpcSystem::new(network, None);
///     let foo_client: foo_client::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
///     tokio::task::spawn_local(rpc_system);
///     Ok(foo_client)
/// }
/// // The reconnecting client that automatically calls connect
/// let (foo_client, _) = auto_reconnect(|| {
///     // By using new_future_client we delay any calls until we have a new connection.
///     Ok(new_future_client(connect()))
/// })?;
/// // Calling Foo like normally.
/// let mut request = foo_client.identity_request();
/// request.get().set_x(123);
/// let promise = request.send().promise.and_then(|response| {
///     println!("results = {}", response.get()?.get_y());
///     Ok(())
/// });
/// ```
pub fn auto_reconnect<F, C>(mut connect: F) -> capnp::Result<(C, Box<dyn SetTarget<C>>)>
where
    F: FnMut() -> capnp::Result<C>,
    F: 'static,
    C: FromClientHook,
    C: 'static,
{
    let current = connect()?;
    let c = Client::new(connect);
    c.set_target(current);
    let hook: Box<dyn ClientHook> = Box::new(c.clone());
    Ok((FromClientHook::new(hook), Box::new(c)))
}

/// Creates a new client that lazily connect and also reconnects when getting [`ErrorKind::Disconnected`](capnp::ErrorKind::Disconnected) errors.
///
/// For explanation of how this functions see: [`auto_reconnect`]
///
/// The main difference between [`auto_reconnect`] and this function is that while [`auto_reconnect`] will call
/// the closure immediately to get an inner client to wrap, this function starts out disconnected and only calls
/// the closure to get the actual client when the capability is first used.
pub fn lazy_auto_reconnect<F, C>(connect: F) -> (C, Box<dyn SetTarget<C>>)
where
    F: FnMut() -> capnp::Result<C>,
    F: 'static,
    C: FromClientHook,
    C: 'static,
{
    let c: Client<F, C> = Client::new(connect);
    let hook: Box<dyn ClientHook> = Box::new(c.clone());
    (FromClientHook::new(hook), Box::new(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::{any_pointer, capability::Client as Capability};
    use std::cell::Cell;

    struct Reenter {
        inner: WeakState,
        replace: RefCell<Option<Box<dyn FnOnce()>>>,
        clones: Rc<Cell<usize>>,
    }
    type WeakState = std::rc::Weak<RefCell<ClientInner<Capability>>>;
    impl ClientHook for Reenter {
        fn add_ref(&self) -> Box<dyn ClientHook> {
            let inner = self.inner.upgrade().unwrap();
            assert!(
                inner.try_borrow_mut().is_ok(),
                "hook cloned under route borrow"
            );
            self.clones.set(self.clones.get() + 1);
            crate::broken::new_cap(capnp::Error::disconnected("old target".into()))
        }
        fn new_call(
            &self,
            interface: u64,
            method: u16,
            size: Option<capnp::MessageSize>,
        ) -> capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
            let replace = self.replace.borrow_mut().take();
            replace.unwrap()();
            crate::broken::new_cap(capnp::Error::disconnected("old target".into()))
                .new_call(interface, method, size)
        }
        fn call(
            &self,
            _: u64,
            _: u16,
            _: Box<dyn capnp::private::capability::ParamsHook>,
            _: Box<dyn capnp::private::capability::ResultsHook>,
        ) -> Promise<(), capnp::Error> {
            unreachable!()
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
        fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, capnp::Error>> {
            None
        }
        fn when_resolved(&self) -> Promise<(), capnp::Error> {
            Promise::ok(())
        }
    }
    #[test]
    fn request_snapshots_route_and_generation_without_cloning_external_hooks() {
        let connects = Rc::new(Cell::new(0));
        let seen = connects.clone();
        let client = Client::<_, Capability>::new(move || {
            seen.set(seen.get() + 1);
            Err(capnp::Error::failed("unexpected reconnect".into()))
        });
        let setter = client.clone();
        let clones = Rc::new(Cell::new(0));
        client.set_target(Capability::new(Box::new(Reenter {
            inner: Rc::downgrade(&client.inner),
            clones: clones.clone(),
            replace: RefCell::new(Some(Box::new(move || {
                setter.set_target(Capability::new(crate::broken::new_cap(
                    capnp::Error::failed("replacement target".into()),
                )));
            }))),
        })));
        let first = client.new_call(0, 0, None).send();
        assert_eq!(
            futures::executor::block_on(first.promise)
                .err()
                .unwrap()
                .kind,
            capnp::ErrorKind::Disconnected
        );
        let second = client.new_call(0, 0, None).send();
        assert_eq!(
            futures::executor::block_on(second.promise)
                .err()
                .unwrap()
                .extra,
            "replacement target"
        );
        assert_eq!(
            connects.get(),
            0,
            "stale failure replaced the current generation"
        );
        assert_eq!(
            clones.get(),
            0,
            "request forwarding allocated an external hook"
        );
    }
}
