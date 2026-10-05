//! Opt-in bulk payload streams; capability calls, cancellation and publication
//! remain on the ordered RPC plane. Run on the session's Tokio LocalSet.
#![forbid(unsafe_code)]
use super::{failed, wire, Completion, Config, Sender, Summary};
use crate::transport::bulk::{Offer, Plane};
use futures::{
    future::{AbortHandle, Abortable},
    FutureExt,
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
#[cfg(test)]
mod tests;

enum Mode {
    Ordinary { used: bool },
    Opening,
    Streaming(Worker),
    Closed,
}
struct Worker {
    abort: AbortHandle,
    done: Completion<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.abort.abort();
    }
}
struct Service {
    client: wire::transfer::Client,
    plane: Plane,
    timeout: Duration,
    mode: RefCell<Mode>,
}

/// Decorate an existing authorized transfer capability. The original receiver
/// still validates every chunk and owns publication. New senders negotiate a
/// stream; old senders use write/done unchanged. A grant is never reusable.
pub fn enable(
    client: wire::transfer::Client,
    plane: Plane,
    timeout: Duration,
) -> capnp::Result<wire::transfer::Client> {
    if timeout.is_zero() || timeout > Duration::from_secs(300) {
        return Err(failed("invalid split transfer deadline"));
    }
    Ok(capnp_rpc::new_client(Service {
        client,
        plane,
        timeout,
        mode: RefCell::new(Mode::Ordinary { used: false }),
    }))
}
impl wire::transfer::Server for Service {
    async fn describe(
        self: Rc<Self>,
        _: wire::transfer::DescribeParams,
        mut out: wire::transfer::DescribeResults,
    ) -> capnp::Result<()> {
        let response = self.client.describe_request().send().promise.await?;
        Config::read(response.get()?.get_config()?)?.write(out.get().init_config());
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        p: wire::transfer::WriteParams,
        mut out: wire::transfer::WriteResults,
    ) -> capnp::Result<()> {
        {
            let mut mode = self.mode.borrow_mut();
            match &mut *mode {
                Mode::Ordinary { used } => *used = true,
                _ => return Err(failed("transfer has selected the stream plane or closed")),
            }
        }
        let p = p.get()?;
        let mut request = self.client.write_request();
        request.get().set_sequence(p.get_sequence());
        request.get().set_data(p.get_data()?);
        out.get()
            .set_sequence(request.send().promise.await?.get()?.get_sequence());
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: wire::transfer::DoneParams,
        mut out: wire::transfer::DoneResults,
    ) -> capnp::Result<()> {
        let completion = {
            let mut mode = self.mode.borrow_mut();
            match &*mode {
                Mode::Opening => return Err(failed("stream negotiation is incomplete")),
                Mode::Streaming(worker) => Some(worker.done.clone()),
                _ => {
                    *mode = Mode::Closed;
                    None
                }
            }
        };
        if let Some(completion) = completion {
            completion.await?;
        }
        let response = self.client.done_request().send().promise.await?;
        out.get().set_summary(response.get()?.get_summary()?)?;
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: wire::transfer::CancelParams,
        mut out: wire::transfer::CancelResults,
    ) -> capnp::Result<()> {
        let previous = self.mode.replace(Mode::Closed);
        drop(previous);
        out.get().set_status(
            self.client
                .cancel_request()
                .send()
                .promise
                .await?
                .get()?
                .get_status()?,
        );
        Ok(())
    }
    async fn open_stream(
        self: Rc<Self>,
        _: wire::transfer::OpenStreamParams,
        mut out: wire::transfer::OpenStreamResults,
    ) -> capnp::Result<()> {
        {
            let mut mode = self.mode.borrow_mut();
            if !matches!(*mode, Mode::Ordinary { used: false }) {
                return Err(failed("bulk stream grant is single-use"));
            }
            *mode = Mode::Opening;
        }
        let response = self.client.describe_request().send().promise.await?;
        let config = Config::read(response.get()?.get_config()?)?;
        let (offer, mut input) = self
            .plane
            .receive(config.length(), self.timeout)
            .map_err(io_error)?;
        let client = self.client.clone();
        let (abort, registration) = AbortHandle::new_pair();
        let (sent, wait) = tokio::sync::oneshot::channel();
        let timeout = self.timeout;
        tokio::task::spawn_local(async move {
            let consume = async {
                let mut buffer = vec![0; config.max_chunk_bytes() as usize];
                let mut remaining = config.length();
                let mut sequence = 0;
                while remaining > 0 {
                    let count = remaining.min(buffer.len() as u64) as usize;
                    input
                        .read_exact(&mut buffer[..count])
                        .await
                        .map_err(io_error)?;
                    sequence += 1;
                    write(&client, sequence, &buffer[..count]).await?;
                    remaining -= count as u64;
                }
                if input.read(&mut [0]).await.map_err(io_error)? != 0 {
                    return Err(failed("excess bulk payload"));
                }
                Ok(())
            };
            let result =
                match tokio::time::timeout(timeout, Abortable::new(consume, registration)).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err(failed("bulk stream consumer canceled")),
                    Err(_) => Err(failed("bulk stream consumer timed out")),
                };
            // Release receive credit immediately on failure; cancellation RPC
            // is best effort and cannot keep a stream or task alive indefinitely.
            drop(input);
            if result.is_err() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    client.cancel_request().send().promise,
                )
                .await;
            }
            let _ = sent.send(result);
        });
        let done = async {
            wait.await
                .map_err(|_| failed("bulk stream worker stopped"))?
        }
        .boxed_local()
        .shared();
        // A cancel may have arrived while describe was awaiting its response.
        if !matches!(*self.mode.borrow(), Mode::Opening) {
            abort.abort();
            return Err(failed("bulk stream negotiation canceled"));
        }
        *self.mode.borrow_mut() = Mode::Streaming(Worker { abort, done });
        out.get().set_offer(&offer.encode());
        Ok(())
    }
}
fn io_error(error: std::io::Error) -> capnp::Error {
    failed(&error.to_string())
}
async fn write(client: &wire::transfer::Client, sequence: u64, bytes: &[u8]) -> capnp::Result<()> {
    let mut request = client.write_request();
    request.get().set_sequence(sequence);
    request.get().set_data(bytes);
    if request.send().promise.await?.get()?.get_sequence() != sequence {
        return Err(failed("mismatched bulk acknowledgment"));
    }
    Ok(())
}

/// Transfer exactly the declared length and await the ordinary done receipt.
/// None selects ordinary RPC (including TCP). Only Unimplemented falls back
/// after negotiation; a rejected or invalid grant never silently downgrades.
/// Dropping this future cancels its stream; receiver deadlines bound cleanup.
pub async fn send(
    client: wire::transfer::Client,
    plane: Option<Plane>,
    mut source: impl AsyncRead + Unpin,
    timeout: Duration,
) -> capnp::Result<Summary> {
    if timeout.is_zero() || timeout > Duration::from_secs(300) {
        return Err(failed("invalid split transfer deadline"));
    }
    let result = tokio::time::timeout(
        timeout,
        send_inner(client.clone(), plane, &mut source, timeout),
    )
    .await
    .map_err(|_| failed("bulk transfer timed out"))
    .and_then(|r| r);
    if result.is_err() {
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            client.cancel_request().send().promise,
        )
        .await;
    }
    result
}
async fn send_inner(
    client: wire::transfer::Client,
    plane: Option<Plane>,
    source: &mut (impl AsyncRead + Unpin),
    timeout: Duration,
) -> capnp::Result<Summary> {
    let mut ordinary = Sender::connect(client.clone()).await?;
    let config = ordinary.config().clone();
    let offer = if let Some(plane) = plane {
        match client.open_stream_request().send().promise.await {
            Ok(response) => {
                let offer = Offer::decode(response.get()?.get_offer()?).map_err(io_error)?;
                if offer.length() != config.length() {
                    return Err(failed("bulk grant length differs from description"));
                }
                Some(plane.send(offer, timeout).map_err(io_error)?)
            }
            Err(error) if error.kind == capnp::ErrorKind::Unimplemented => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let mut stream = offer;
    let mut buffer = vec![0; config.max_chunk_bytes() as usize];
    let mut remaining = config.length();
    let mut chunks = 0;
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        source
            .read_exact(&mut buffer[..count])
            .await
            .map_err(io_error)?;
        if let Some(stream) = &mut stream {
            stream.write_all(&buffer[..count]).await.map_err(io_error)?;
        } else {
            ordinary.write(&buffer[..count]).await?;
        }
        remaining -= count as u64;
        chunks += 1;
    }
    if source.read(&mut [0]).await.map_err(io_error)? != 0 {
        return Err(failed("source exceeds declared bulk length"));
    }
    if let Some(stream) = stream {
        stream.finish().await.map_err(io_error)?;
        let response = client.done_request().send().promise.await?;
        let summary = response.get()?.get_summary()?;
        let actual = Summary {
            bytes: summary.get_bytes(),
            chunks: summary.get_chunks(),
        };
        if actual
            != (Summary {
                bytes: config.length(),
                chunks,
            })
        {
            return Err(failed("mismatched bulk completion"));
        }
        Ok(actual)
    } else {
        ordinary.done().await
    }
}
