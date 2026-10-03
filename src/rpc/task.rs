use super::Completion;
use capnp::{capability::Promise, Error};
use futures::{channel::oneshot, FutureExt};
use std::future::Future;

/// One task owner; observers never own the driver. Dropping the owner schedules
/// cancellation even when startup or shutdown has not yet been polled.
pub(crate) struct Task {
    task: tokio::task::JoinHandle<()>,
    stop: Option<oneshot::Sender<capnp::Result<()>>>,
    completion: Completion,
}

impl Task {
    pub(crate) fn spawn(driver: impl Future<Output = capnp::Result<()>> + 'static) -> Self {
        let (stop, stopped) = oneshot::channel();
        let (done, receiver) = oneshot::channel();
        let completion = Promise::from_future(async move {
            receiver
                .await
                .map_err(|_| Error::disconnected("RPC task canceled or panicked".into()))?
        })
        .shared();
        let task = tokio::task::spawn_local(async move {
            let result = {
                let driver = driver;
                tokio::pin!(driver);
                tokio::select! {
                    biased;
                    result = stopped => result.unwrap_or_else(|_| {
                        Err(Error::disconnected("RPC owner dropped".into()))
                    }),
                    result = &mut driver => result,
                }
            }; // Drop the driver and its transports before notifying observers.
            let _ = done.send(result);
        });
        Self {
            task,
            stop: Some(stop),
            completion,
        }
    }

    pub(crate) fn completion(&self) -> Completion {
        self.completion.clone()
    }

    pub(crate) async fn finish(&mut self, result: capnp::Result<()>) -> capnp::Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(result);
        }
        // Waiting for cleanup also makes this safe when the driver ended first.
        let _ = (&mut self.task).await;
        self.completion.clone().await
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.task.abort();
    }
}
