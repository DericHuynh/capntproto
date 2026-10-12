//! Automatic registration and managed IO for existing VatNetwork connections.
use super::*;
use transport::vat;

/// A registered portable driver and its message-oriented transport. Identity
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
        let (mut reader, mut writer, failure) = self
            .transport
            .into_io(self.registered.0.options.input_bytes);
        let mut failure = std::pin::pin!(failure);
        let mut abort = std::pin::pin!(abort);
        let aborted = std::future::poll_fn(|cx| {
            // Explicit deadlines/cancellation retain priority over queue failure.
            if let Poll::Ready(error) = abort.as_mut().poll(cx) {
                return Poll::Ready(error);
            }
            failure.as_mut().poll(cx)
        });
        self.registered
            .run_connection(
                messages,
                queue,
                &mut reader,
                &mut writer,
                scratch,
                resources,
                controls,
                shutdown,
                aborted,
            )
            .await
    }
}
