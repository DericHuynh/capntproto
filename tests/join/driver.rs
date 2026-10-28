//! Replay the same bilateral conversations with either protocol implementation.
use super::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
pub struct System<V: 'static, const PORTABLE: bool> {
    system: Option<support::driver::System<V>>,
    running: Option<Pin<Box<dyn Future<Output = capnp::Result<()>>>>>,
}
impl<V: 'static, const PORTABLE: bool> System<V, PORTABLE> {
    pub fn new(
        network: Box<dyn capnp_rpc::VatNetwork<V>>,
        bootstrap: Option<capnp::capability::Client>,
    ) -> Self {
        Self {
            system: Some(support::driver::System::new(PORTABLE, network, bootstrap)),
            running: None,
        }
    }
    pub fn new_with_bootstrap_factory(
        network: Box<dyn capnp_rpc::VatNetwork<V>>,
        local: V,
        factory: Rc<dyn capnp_rpc::BootstrapFactory<V>>,
    ) -> Self {
        Self {
            system: Some(support::driver::System::with_factory(
                PORTABLE, network, local, factory,
            )),
            running: None,
        }
    }
    pub fn bootstrap<T: capnp::capability::FromClientHook>(&mut self, peer: V) -> T {
        self.system
            .as_mut()
            .expect("configure before running")
            .bootstrap(peer)
    }
    pub fn get_joiner(&self) -> support::driver::Joiner<V> {
        self.system
            .as_ref()
            .expect("configure before running")
            .get_joiner()
    }
    pub fn get_disconnector(&self) -> capnp::capability::Promise<(), capnp::Error> {
        self.system
            .as_ref()
            .expect("configure before running")
            .disconnect()
    }
}
impl<V: 'static, const PORTABLE: bool> Future for System<V, PORTABLE> {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(system) = self.system.take() {
            self.running = Some(Box::pin(system.run()));
        }
        self.running
            .as_mut()
            .expect("running system")
            .as_mut()
            .poll(cx)
    }
}
