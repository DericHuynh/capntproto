//! Exercise identical network scenarios through the migration driver.
#![allow(dead_code)]
use super::*;
pub enum System<V: 'static = u8> {
    Hosted(capnp_rpc::RpcSystem<V>),
    Portable(capnp_rpc::portable::RpcSystem<V>),
}
impl<V: 'static> System<V> {
    pub fn get_joiner(&self) -> Joiner<V> {
        match self {
            Self::Hosted(system) => Joiner::Hosted(system.get_joiner()),
            Self::Portable(system) => Joiner::Portable(system.get_joiner()),
        }
    }
    pub fn set_flow_limit(&mut self, words: usize) {
        match self {
            Self::Hosted(system) => system.set_flow_limit(words),
            Self::Portable(system) => system.control().set_flow_limit(words),
        }
    }
    pub fn set_outgoing_call_limit(&mut self, calls: usize) {
        match self {
            Self::Hosted(system) => system.set_outgoing_call_limit(calls),
            Self::Portable(system) => system.control().set_outgoing_call_limit(calls),
        }
    }
    pub fn disconnect(&self) -> Promise<(), Error> {
        match self {
            Self::Hosted(system) => Promise::from_future(system.get_disconnector()),
            Self::Portable(system) => Promise::from_future(system.get_disconnector()),
        }
    }
    pub fn new(
        portable: bool,
        network: Box<dyn VatNetwork<V>>,
        bootstrap: Option<capnp::capability::Client>,
    ) -> Self {
        Self::with_options(portable, network, bootstrap, options())
    }
    pub fn with_options(
        portable: bool,
        network: Box<dyn VatNetwork<V>>,
        bootstrap: Option<capnp::capability::Client>,
        options: capnp_rpc::portable::Options,
    ) -> Self {
        if portable {
            Self::Portable(
                capnp_rpc::portable::RpcSystem::new(network, bootstrap, options).unwrap(),
            )
        } else {
            Self::Hosted(capnp_rpc::RpcSystem::new(network, bootstrap))
        }
    }
    pub fn with_factory(
        portable: bool,
        network: Box<dyn VatNetwork<V>>,
        local_id: V,
        factory: Rc<dyn capnp_rpc::BootstrapFactory<V>>,
    ) -> Self {
        Self::with_factory_options(portable, network, local_id, factory, options())
    }
    pub fn with_factory_options(
        portable: bool,
        network: Box<dyn VatNetwork<V>>,
        local_id: V,
        factory: Rc<dyn capnp_rpc::BootstrapFactory<V>>,
        options: capnp_rpc::portable::Options,
    ) -> Self {
        if portable {
            Self::Portable(
                capnp_rpc::portable::RpcSystem::new_with_bootstrap_factory(
                    network, local_id, factory, options,
                )
                .unwrap(),
            )
        } else {
            Self::Hosted(capnp_rpc::RpcSystem::new_with_bootstrap_factory(
                network, local_id, factory,
            ))
        }
    }
    pub fn bootstrap<T: capnp::capability::FromClientHook>(&mut self, id: V) -> T {
        match self {
            Self::Hosted(s) => s.bootstrap(id),
            Self::Portable(s) => s.bootstrap(id),
        }
    }
    pub async fn run(self) -> capnp::Result<()> {
        match self {
            Self::Hosted(s) => s.await,
            Self::Portable(s) => s.run().await,
        }
    }

    /// Keep the same control operations available while either driver runs.
    /// The hosted driver retains its original shared poll owner for tests of
    /// reentrant bootstrap and disconnect callbacks.
    pub fn start(
        self,
    ) -> (
        RunningSystem<V>,
        futures::future::LocalBoxFuture<'static, capnp::Result<()>>,
    ) {
        match self {
            Self::Hosted(system) => {
                let system = Rc::new(RefCell::new(system));
                let driver = system.clone();
                (
                    RunningSystem::Hosted(system),
                    Box::pin(futures::future::poll_fn(move |cx| {
                        std::future::Future::poll(std::pin::Pin::new(&mut *driver.borrow_mut()), cx)
                    })),
                )
            }
            Self::Portable(system) => {
                let running = RunningSystem::Portable {
                    bootstrapper: system.get_bootstrapper(),
                    control: system.control(),
                    disconnect: futures::FutureExt::shared(Promise::from_future(
                        system.get_disconnector(),
                    )),
                };
                (running, Box::pin(system.run()))
            }
        }
    }
}

pub enum RunningSystem<V: 'static = u8> {
    Hosted(Rc<RefCell<capnp_rpc::RpcSystem<V>>>),
    Portable {
        bootstrapper: capnp_rpc::portable::Bootstrapper<V>,
        control: capnp_rpc::portable::Control,
        disconnect: futures::future::Shared<Promise<(), Error>>,
    },
}
impl<V: 'static> RunningSystem<V> {
    pub fn bootstrap<T: capnp::capability::FromClientHook>(&mut self, id: V) -> T {
        match self {
            Self::Hosted(system) => system.borrow_mut().bootstrap(id),
            Self::Portable { bootstrapper, .. } => bootstrapper.bootstrap(id),
        }
    }
    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        match self {
            Self::Hosted(system) => system.borrow_mut().set_trace_encoder(encoder),
            Self::Portable { control, .. } => control.set_trace_encoder(encoder),
        }
    }
    pub fn clear_trace_encoder(&mut self) {
        match self {
            Self::Hosted(system) => system.borrow_mut().clear_trace_encoder(),
            Self::Portable { control, .. } => control.clear_trace_encoder(),
        }
    }
    pub fn get_disconnector(&self) -> Promise<(), Error> {
        match self {
            Self::Hosted(system) => Promise::from_future(system.borrow().get_disconnector()),
            Self::Portable { disconnect, .. } => Promise::from_future(disconnect.clone()),
        }
    }
}

pub enum Joiner<V: 'static> {
    Hosted(capnp_rpc::Joiner<V>),
    Portable(capnp_rpc::portable::Joiner<V>),
}
impl<V: 'static> Joiner<V> {
    pub fn join<T: capnp::capability::FromClientHook + 'static>(
        &self,
        caps: Vec<T>,
    ) -> Promise<Option<T>, Error> {
        match self {
            Self::Hosted(joiner) => joiner.join(caps),
            Self::Portable(joiner) => joiner.join(caps),
        }
    }
}

pub fn options() -> capnp_rpc::portable::Options {
    capnp_rpc::portable::Options::bounded(
        16.try_into().unwrap(),
        256.try_into().unwrap(),
        (512 * 1024).try_into().unwrap(),
        (8 * 1024 * 1024).try_into().unwrap(),
    )
    .unwrap()
}
