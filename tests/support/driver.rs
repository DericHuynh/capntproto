//! Legacy RPC test harness; no backend selection.
#![allow(dead_code)]
use super::*;
type Driver = std::pin::Pin<Box<dyn std::future::Future<Output = capnp::Result<()>>>>;
pub struct System<V: 'static = u8>(pub capnp_rpc::RpcSystem<V>);
impl<V: 'static> std::ops::Deref for System<V> {
    type Target = capnp_rpc::RpcSystem<V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<V: 'static> std::ops::DerefMut for System<V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<V: 'static> System<V> {
    pub fn new(
        network: Box<dyn VatNetwork<V>>,
        bootstrap: Option<capnp::capability::Client>,
    ) -> Self {
        Self(capnp_rpc::RpcSystem::new(network, bootstrap))
    }
    pub fn with_factory(
        network: Box<dyn VatNetwork<V>>,
        id: V,
        factory: Rc<dyn capnp_rpc::BootstrapFactory<V>>,
    ) -> Self {
        Self(capnp_rpc::RpcSystem::new_with_bootstrap_factory(
            network, id, factory,
        ))
    }
    pub fn disconnect(&self) -> Promise<(), Error> {
        Promise::from_future(self.0.get_disconnector())
    }
    pub async fn run(self) -> capnp::Result<()> {
        self.0.await
    }
    pub fn start(self) -> (RunningSystem<V>, Driver) {
        let driver = Rc::new(RefCell::new(self.0));
        (
            RunningSystem(driver.clone()),
            Box::pin(futures::future::poll_fn(move |cx| {
                std::future::Future::poll(std::pin::Pin::new(&mut *driver.borrow_mut()), cx)
            })),
        )
    }
}
pub struct RunningSystem<V: 'static>(Rc<RefCell<capnp_rpc::RpcSystem<V>>>);
impl<V: 'static> RunningSystem<V> {
    pub fn bootstrap<T: capnp::capability::FromClientHook>(&mut self, id: V) -> T {
        self.0.borrow_mut().bootstrap(id)
    }
    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        self.0.borrow_mut().set_trace_encoder(encoder);
    }
    pub fn clear_trace_encoder(&mut self) {
        self.0.borrow_mut().clear_trace_encoder();
    }
    pub fn get_disconnector(&self) -> Promise<(), Error> {
        Promise::from_future(self.0.borrow().get_disconnector())
    }
}
pub type Joiner<V> = capnp_rpc::Joiner<V>;
