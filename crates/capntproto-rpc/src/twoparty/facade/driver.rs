//! Driver selection stays outside the protocol and transport hot paths.
use super::*;

/// Protocol implementation for the client/server conveniences. Legacy remains
/// the default; portable uses explicit bounded storage and never falls back.
#[derive(Clone, Copy, Default)]
pub enum RpcDriver {
    #[default]
    Legacy,
    Portable(crate::portable::Options),
}

impl RpcDriver {
    pub(super) fn reader_options(self) -> ReaderOptions {
        match self {
            Self::Legacy => ReaderOptions::new(),
            Self::Portable(options) => options.reader,
        }
    }
    pub(super) fn with_reader_options(mut self, reader: ReaderOptions) -> Self {
        if let Self::Portable(options) = &mut self {
            options.reader = reader;
        }
        self
    }
}

// Preserve the default legacy driver's inline storage and allocation count.
#[allow(clippy::large_enum_variant)]
pub(super) enum Running {
    Legacy(RpcSystem<VatId>),
    Portable(Portable),
}
pub(super) struct Portable {
    future: Pin<Box<dyn Future<Output = capnp::Result<()>>>>,
    bootstrap: crate::portable::Bootstrapper<VatId>,
    control: crate::portable::Control,
    disconnect: Shared<Promise<(), Error>>,
    diagnostics: crate::RpcDiagnostics,
}
impl Running {
    pub(super) fn new(
        network: Box<dyn crate::VatNetwork<VatId>>,
        bootstrap: Option<Client>,
        driver: RpcDriver,
    ) -> capnp::Result<Self> {
        match driver {
            RpcDriver::Legacy => Ok(Self::Legacy(RpcSystem::new(network, bootstrap))),
            RpcDriver::Portable(options) => {
                let system = crate::portable::RpcSystem::new(network, bootstrap, options)?;
                let bootstrap = system.get_bootstrapper();
                let control = system.control();
                let disconnect = Promise::from_future(system.get_disconnector()).shared();
                let diagnostics = system.diagnostics();
                Ok(Self::Portable(Portable {
                    future: Box::pin(system.run()),
                    bootstrap,
                    control,
                    disconnect,
                    diagnostics,
                }))
            }
        }
    }
    pub(super) fn bootstrap<T: capnp::capability::FromClientHook>(&mut self, peer: VatId) -> T {
        match self {
            Self::Legacy(system) => system.bootstrap(peer),
            Self::Portable(system) => system.bootstrap.bootstrap(peer),
        }
    }
    pub(super) fn get_disconnector(&self) -> Promise<(), Error> {
        match self {
            Self::Legacy(system) => Promise::from_future(system.get_disconnector()),
            Self::Portable(system) => Promise::from_future(system.disconnect.clone()),
        }
    }
    pub(super) fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        match self {
            Self::Legacy(system) => system.set_trace_encoder(encoder),
            Self::Portable(system) => system.control.set_trace_encoder(encoder),
        }
    }
    pub(super) fn clear_trace_encoder(&mut self) {
        match self {
            Self::Legacy(system) => system.clear_trace_encoder(),
            Self::Portable(system) => system.control.clear_trace_encoder(),
        }
    }
    pub(super) fn set_outgoing_call_limit(&mut self, calls: usize) {
        match self {
            Self::Legacy(system) => system.set_outgoing_call_limit(calls),
            Self::Portable(system) => system.control.set_outgoing_call_limit(calls),
        }
    }
    pub(super) fn diagnostics(&self) -> crate::RpcDiagnostics {
        match self {
            Self::Legacy(system) => system.diagnostics(),
            Self::Portable(system) => system.diagnostics.clone(),
        }
    }
    pub(super) fn is_closing(&self) -> bool {
        match self {
            Self::Legacy(system) => system.connections.borrow().is_closing(),
            Self::Portable(system) => system.control.is_closing(),
        }
    }
}
impl Future for Running {
    type Output = capnp::Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut() {
            Self::Legacy(system) => Pin::new(system).poll(cx),
            Self::Portable(system) => system.future.as_mut().poll(cx),
        }
    }
}
