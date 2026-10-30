//! Automatic registration and managed IO for existing VatNetwork connections.
use super::*;
use transport::vat;

/// A registered portable driver and its exclusive frame transport. Identity
/// and peer authorization come from the supplied connection, never wire data.
/// Drive the originating VatNetwork concurrently with run.
#[must_use = "drive this connection or end its entire message-pool scope"]
pub struct VatConnection<C, V> {
    registered: Registered<C>,
    transport: vat::Connection<V>,
}
impl<'p, 'id, 's, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, resources::Import, P, C, H>
{
    /// Adapt and register an existing transport without changing its network
    /// driver. Rejection returns both unchanged owners for recovery or teardown.
    pub fn register_vat<V>(
        self,
        network: &Network,
        connection: Box<dyn crate::Connection<V>>,
    ) -> Result<VatConnection<Self, V>, Box<Rejected<(Self, vat::Connection<V>)>>> {
        let transport = vat::Connection::new(connection);
        match self.register(network, transport.id()) {
            Ok(registered) => Ok(VatConnection {
                registered,
                transport,
            }),
            Err(rejected) => Err(Box::new(Rejected {
                error: rejected.error,
                coordinator: (rejected.coordinator, transport),
            })),
        }
    }
}
impl<'p, 'id, 's, P: Copy + Eq, V, const C: usize, const H: usize>
    VatConnection<Coordinator<'p, 'id, 's, resources::Import, P, C, H>, V>
{
    pub fn handle(&self) -> Handle {
        self.registered.handle()
    }
    pub fn peer(&self) -> &V {
        self.transport.peer()
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        resources: &vat::Resources,
        controls: &mut impl Control<'p, 'id, 's, resources::Import, P, C, H, vat::Resources>,
        shutdown: impl Future<Output = capnp::Result<()>>,
        abort: impl Future<Output = Error>,
    ) -> Result<(), RunError<Error, Error>> {
        let (mut reader, mut writer, failure) = match self
            .transport
            .into_io(self.registered.0.options.input_bytes)
        {
            Ok(io) => io,
            Err(error) => {
                return self
                    .registered
                    .0
                    .terminate::<vat::Resources, Error, Error>(
                        messages,
                        queue,
                        controls,
                        Err(Cause::Protocol(error)),
                    )
                    .await
            }
        };
        self.registered
            .0
            .run_vat(
                messages,
                queue,
                &mut reader,
                &mut writer,
                scratch,
                resources,
                controls,
                shutdown,
                abort,
                failure,
            )
            .await
    }
}
impl<'p, 'id, 's, P: Copy + Eq, const C: usize, const H: usize>
    Coordinator<'p, 'id, 's, resources::Import, P, C, H>
{
    // Both hosted entry points use the same teardown ownership and failure
    // observation. Native output errors stop work without canceling close.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run_vat(
        self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        reader: &mut vat::Reader,
        writer: &mut vat::Writer,
        scratch: &mut [u32],
        resources: &vat::Resources,
        controls: &mut impl Control<'p, 'id, 's, resources::Import, P, C, H, vat::Resources>,
        shutdown: impl Future<Output = capnp::Result<()>>,
        abort: impl Future<Output = Error>,
        failure: vat::Failure,
    ) -> Result<(), RunError<Error, Error>> {
        let mut failure = std::pin::pin!(failure);
        let mut abort = std::pin::pin!(abort);
        let shutdown = std::pin::pin!(shutdown);
        let mut shutdown = std::pin::pin!(notified::Notified::new(shutdown));
        let mut caller_aborted = false;
        let (mut closing, result) = {
            let aborted = std::pin::pin!(std::future::poll_fn(|cx| {
                // Explicit deadlines/cancellation retain priority over queue failure.
                if let Poll::Ready(error) = abort.as_mut().poll(cx) {
                    caller_aborted = true;
                    return Poll::Ready(error);
                }
                failure.as_mut().poll(cx)
            }));
            let mut aborted = std::pin::pin!(notified::Notified::new(aborted));
            self.prepare_connection(
                messages,
                queue,
                reader,
                writer,
                scratch,
                resources,
                controls,
                shutdown.as_mut(),
                aborted.as_mut(),
            )
            .await
        };
        // Queue failure ends protocol work, but does not cancel physical close.
        // Only the caller's explicit abort/deadline may skip further IO.
        closing.hard_abort = caller_aborted;
        writer.shutdown_reason(closing.reason.clone());
        closing
            .finish(messages, queue, writer, abort.as_mut(), result)
            .await
    }
}
