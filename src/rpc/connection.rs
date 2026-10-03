use super::{task::Task, Completion};
use capnp::capability::{Client, FromClientHook, Promise};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::TwoPartyClient};
use std::time::Duration;
use tokio_util::compat::TokioAsyncReadCompatExt;

/// An owned, running bilateral RPC connection on a Tokio `LocalSet`.
///
/// Dropping the owner cancels its task. Bootstrap clients and disconnect
/// observers can outlive the owner without keeping its driver running.
/// Use [`Self::shutdown`] for bounded protocol shutdown and completed cleanup.
#[must_use = "keep the connection owner alive while using its remote capabilities"]
pub struct Connection {
    bootstrap: Client,
    disconnect: Option<Promise<(), capnp::Error>>,
    queue: capnp_rpc::twoparty::QueueDiagnostics,
    diagnostics: capnp_rpc::RpcDiagnostics,
    task: Task,
}

impl Connection {
    /// Start RPC on generic Tokio byte IO. For socket-aware flow control or
    /// configured reader limits, use a transport's client adapter and `spawn`.
    /// Panics outside a Tokio local executor, like `spawn_local`.
    pub fn new(
        io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
        bootstrap: Option<Client>,
        side: Side,
    ) -> Self {
        Self::spawn(TwoPartyClient::with_bootstrap(io.compat(), bootstrap, side))
    }

    /// Start a configured TCP, TLS, QUIC or other owned two-party driver.
    /// Panics outside a Tokio local executor, like `spawn_local`.
    pub fn spawn(mut driver: TwoPartyClient<'static>) -> Self {
        Self {
            bootstrap: driver.bootstrap(),
            disconnect: Some(driver.get_disconnector()),
            queue: driver.outgoing_queue(),
            diagnostics: driver.diagnostics(),
            task: Task::spawn(driver),
        }
    }

    /// The remote bootstrap. Requests can be pipelined without a readiness wait.
    pub fn bootstrap<T: FromClientHook>(&self) -> T {
        T::new(self.bootstrap.hook.add_ref())
    }

    /// Shared completion after driver cleanup; dropping an observer is harmless.
    pub fn on_disconnect(&self) -> Completion {
        self.task.completion()
    }

    pub fn outgoing_queue(&self) -> capnp_rpc::twoparty::QueueDiagnostics {
        self.queue.clone()
    }

    pub fn diagnostics(&self) -> capnp_rpc::RpcDiagnostics {
        self.diagnostics.clone()
    }

    /// Close RPC and wait for cleanup within the supplied protocol deadline.
    /// Stop issuing calls first: this does not wait for application turns to
    /// finish or promise durable execution. Canceling this future drops the owner
    /// and aborts the driver. A zero timeout is rejected and closes the owner.
    pub async fn shutdown(mut self, timeout: Duration) -> capnp::Result<()> {
        let result = if timeout.is_zero() {
            Err(capnp::Error::failed(
                "RPC shutdown timeout must be positive".into(),
            ))
        } else {
            tokio::time::timeout(timeout, self.disconnect.take().unwrap())
                .await
                .unwrap_or_else(|_| {
                    Err(capnp::Error::disconnected("RPC shutdown timed out".into()))
                })
        };
        let completed = self.task.finish(result.clone()).await;
        result.and(completed)
    }
}
