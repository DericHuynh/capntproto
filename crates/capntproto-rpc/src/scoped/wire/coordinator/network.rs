//! Hosted network ownership over the same registry used by default RpcSystem.
//! The caller supplies authenticated connections, scoped storage and IO; this
//! layer owns neither protocol tables nor a transport/executor.
use super::*;
use crate::system::registry::{self, Acquired, Connections};
use futures::future::Shared;
mod vat;
pub use vat::VatConnection;

struct State {
    handle: Handle,
    stop: RefCell<Option<shared::Sender<capnp::Result<()>>>>,
}
impl State {
    fn stop(&self, result: capnp::Result<()>) {
        let sender = self.stop.borrow_mut().take();
        let reason = result
            .as_ref()
            .err()
            .cloned()
            .unwrap_or_else(|| Error::disconnected("coordinator network shutting down".into()));
        if let Some(sender) = sender {
            let _ = sender.send(result);
        }
        self.handle.close(reason);
    }
}
impl registry::Connection for State {
    fn disconnect(&self, error: Error) {
        self.stop(Err(error));
    }
}

/// Hosted registry for caller-driven portable coordinators. IDs are trusted
/// transport identities: distinct live connections need distinct IDs, even for
/// the same peer. This registry does not authenticate peers or implement dialing.
/// Register before starting IO; poll each returned Registered connection yourself.
/// Dropping the network requests orderly shutdown; it does not drive that IO.
pub struct Network {
    connections: Connections<State>,
}
impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}
impl Network {
    pub fn new() -> Self {
        Self {
            connections: Connections::new(),
        }
    }
    /// Lookup cannot revive detached/closing connections. Returned handles are weak.
    pub fn connection(&self, id: usize) -> Option<Handle> {
        if self.connections.is_closing() {
            return None;
        }
        self.connections.get(id).map(|state| state.handle.clone())
    }
    /// Start orderly shutdown on first poll and await all IO/protocol cleanup.
    /// Independent observers all receive completion and the first failure.
    pub fn get_disconnector(&self) -> Disconnector {
        Disconnector {
            connections: self.connections.clone(),
            closed: self.connections.closed(),
        }
    }
    fn register(&self, id: usize, handle: Handle) -> capnp::Result<Registration> {
        let guard = match self.connections.acquire(id)? {
            Acquired::Existing(_) => {
                return Err(Error::overloaded(
                    "connection identity already registered".into(),
                ))
            }
            Acquired::New(guard) => guard,
        };
        let (sender, stop) = shared::channel();
        guard.publish(Rc::new(State {
            handle,
            stop: RefCell::new(Some(sender)),
        }))?;
        Ok(Registration { guard, stop })
    }
}
fn close(connections: &Connections<State>) {
    for state in connections.begin_close() {
        state.stop(Ok(()));
    }
}
impl Drop for Network {
    fn drop(&mut self) {
        close(&self.connections);
    }
}

#[must_use = "poll while driving every registered coordinator"]
pub struct Disconnector {
    connections: Connections<State>,
    closed: Shared<capability::Promise<(), Error>>,
}
impl Future for Disconnector {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        close(&self.connections);
        Pin::new(&mut self.closed).poll(cx)
    }
}

pub(super) struct Registration {
    guard: registry::Registration<State>,
    stop: shared::Receiver<capnp::Result<()>>,
}
impl Registration {
    pub(super) fn complete_physical<R, W>(self, result: &Result<(), RunError<R, W>>, closed: bool) {
        if closed
            && result
                .as_ref()
                .err()
                .is_none_or(|error| error.cleanup.is_none())
        {
            self.guard.complete(Ok(()));
        } else {
            self.complete(result);
        }
    }
    pub(super) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        Pin::new(&mut self.stop).poll(cx).map(|result| {
            result.unwrap_or_else(|_| {
                Err(Error::disconnected("network registration canceled".into()))
            })
        })
    }
    pub(super) fn detach(&self) {
        self.guard.detach();
    }
    pub(super) fn complete<R, W>(self, result: &Result<(), RunError<R, W>>) {
        // Preserve protocol/deadline reasons without imposing Debug on caller IO
        // errors. The running caller still receives its original typed RunError.
        let outcome = result.as_ref().map(|_| ()).map_err(|error| {
            if error.close.is_some() {
                Error::disconnected("registered connection close failed".into())
            } else {
                match &error.cause {
                    Some(Cause::Protocol(error)) => error.clone(),
                    Some(Cause::Input(_)) => {
                        Error::disconnected("registered connection input failed".into())
                    }
                    Some(Cause::Output(_)) => {
                        Error::disconnected("registered connection output failed".into())
                    }
                    None => Error::failed("registered connection protocol cleanup failed".into()),
                }
            }
        });
        self.guard.complete(outcome);
    }
}

/// Registration rejection returns the unchanged coordinator for retry or cleanup.
pub struct Rejected<C> {
    pub error: Error,
    pub coordinator: C,
}
impl<C> std::fmt::Debug for Rejected<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rejected")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// A registered connection exposes only managed execution. Immediate-stop APIs
/// cannot accidentally report drained/closed IO to its network. Dropping this
/// owner (even before its future's first poll) reports cancellation. As with an
/// unregistered coordinator, early drop requires ending its entire pool scope.
#[must_use = "drive registered IO to completion, or end its entire pool scope"]
pub struct Registered<C>(C);

impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, I, P, C, H>
{
    pub fn register(
        mut self,
        network: &Network,
        id: usize,
    ) -> Result<Registered<Self>, Box<Rejected<Self>>> {
        match network.register(id, self.handle()) {
            Ok(registration) => {
                self.registration = Some(registration);
                Ok(Registered(self))
            }
            Err(error) => Err(Box::new(Rejected {
                error,
                coordinator: self,
            })),
        }
    }
}
impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    Registered<Coordinator<'p, 'id, 's, I, P, C, H>>
{
    pub fn handle(&self) -> Handle {
        self.0.handle()
    }

    /// Managed byte IO with independent caller shutdown and abort/deadline inputs.
    /// Network shutdown is an additional orderly-stop input to the same driver.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_stream<R: io::Read, W: io::Write + io::Close>(
        self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        make_import: impl FnMut(ImportId) -> I,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H>,
        shutdown: impl Future<Output = capnp::Result<()>>,
        abort: impl Future<Output = Error>,
    ) -> Result<(), RunError<R::Error, W::Error>> {
        self.0
            .run_stream(
                messages,
                queue,
                reader,
                writer,
                scratch,
                make_import,
                controls,
                shutdown,
                abort,
            )
            .await
    }

    /// Managed resource IO; registration lasts through drain/close and cleanup.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_connection<
        O: Output<I>,
        R: Input<I>,
        W: io::WriteResources<O::Bundle> + io::Close,
    >(
        self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        reader: &mut R,
        writer: &mut W,
        scratch: &mut [u32],
        output: &O,
        controls: &mut impl Control<'p, 'id, 's, I, P, C, H, O>,
        shutdown: impl Future<Output = capnp::Result<()>>,
        abort: impl Future<Output = Error>,
    ) -> Result<(), RunError<R::Error, W::Error>> {
        self.0
            .run_connection(
                messages, queue, reader, writer, scratch, output, controls, shutdown, abort,
            )
            .await
    }
}
