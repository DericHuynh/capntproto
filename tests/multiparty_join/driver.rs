//! Run identical authenticated-network assertions through both implementations.
use super::*;
pub enum System {
    Hosted(capnp_rpc::RpcSystem<Vat>),
    Portable(capnp_rpc::portable::RpcSystem<Vat>),
}
fn options() -> capnp_rpc::portable::Options {
    capnp_rpc::portable::Options::bounded(
        16.try_into().unwrap(),
        256.try_into().unwrap(),
        (512 * 1024).try_into().unwrap(),
        (8 * 1024 * 1024).try_into().unwrap(),
    )
    .unwrap()
}
impl System {
    pub fn new(
        portable: bool,
        network: Box<dyn VatNetwork<Vat>>,
        bootstrap: Option<capnp::capability::Client>,
    ) -> Self {
        if portable {
            Self::Portable(
                capnp_rpc::portable::RpcSystem::new(network, bootstrap, options()).unwrap(),
            )
        } else {
            Self::Hosted(capnp_rpc::RpcSystem::new(network, bootstrap))
        }
    }
    pub fn new_with_bootstrap_factory(
        portable: bool,
        network: Box<dyn VatNetwork<Vat>>,
        id: Vat,
        factory: Rc<dyn capnp_rpc::BootstrapFactory<Vat>>,
    ) -> Self {
        if portable {
            Self::Portable(
                capnp_rpc::portable::RpcSystem::new_with_bootstrap_factory(
                    network,
                    id,
                    factory,
                    options(),
                )
                .unwrap(),
            )
        } else {
            Self::Hosted(capnp_rpc::RpcSystem::new_with_bootstrap_factory(
                network, id, factory,
            ))
        }
    }
    pub fn bootstrap<T: capnp::capability::FromClientHook>(&mut self, peer: Vat) -> T {
        match self {
            Self::Hosted(system) => system.bootstrap(peer),
            Self::Portable(system) => system.bootstrap(peer),
        }
    }
    pub fn get_joiner(&self) -> Joiner {
        match self {
            Self::Hosted(system) => Joiner::Hosted(system.get_joiner()),
            Self::Portable(system) => Joiner::Portable(system.get_joiner()),
        }
    }
    pub async fn run(self) -> capnp::Result<()> {
        match self {
            Self::Hosted(system) => system.await,
            Self::Portable(system) => system.run().await,
        }
    }
}
pub enum Joiner {
    Hosted(capnp_rpc::Joiner<Vat>),
    Portable(capnp_rpc::portable::Joiner<Vat>),
}
impl Joiner {
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
