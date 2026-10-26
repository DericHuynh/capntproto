use super::*;
use crate::grpc::{access_envelope::Body, AccessEnvelope, DescribeRequest, Description};
use futures::{stream::FuturesUnordered, FutureExt, SinkExt, StreamExt};
use prost::Message as _;
use std::collections::HashMap;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message},
    WebSocketStream,
};

const IN_FLIGHT: usize = 64;
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(8 * 1024 * 1024))
        .max_frame_size(Some(8 * 1024 * 1024))
}
type Reply = oneshot::Sender<std::result::Result<Body, String>>;
struct Command {
    body: Body,
    reply: Reply,
    permit: tokio::sync::OwnedSemaphorePermit,
}

// A bounded mailbox and one connection owner preserve framing under concurrent
// callers. The correlation table supports independently completing service work.
#[derive(Clone)]
pub(super) struct Client {
    commands: mpsc::Sender<Command>,
    permits: std::sync::Arc<tokio::sync::Semaphore>,
}
impl Client {
    pub async fn connect(address: &str) -> Result<Self> {
        let socket = crate::benchmark_tls::connect(address).await?;
        let (socket, _) = tokio_tungstenite::client_async_with_config(
            format!("wss://{address}/access"),
            socket,
            Some(config()),
        )
        .await?;
        Ok(Self::from_socket(socket))
    }
    fn from_socket<S: AsyncRead + AsyncWrite + Unpin + 'static>(
        socket: WebSocketStream<S>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(IN_FLIGHT);
        tokio::task::spawn_local(drive(socket, rx));
        Self {
            commands: tx,
            permits: std::sync::Arc::new(tokio::sync::Semaphore::new(IN_FLIGHT)),
        }
    }
    async fn request(&self, body: Body) -> Result<Body> {
        let (reply, response) = oneshot::channel();
        let permit = self.permits.clone().acquire_owned().await?;
        self.commands
            .send(Command {
                body,
                reply,
                permit,
            })
            .await
            .map_err(|_| "WebSocket connection stopped")?;
        response
            .await
            .map_err(|_| "WebSocket connection stopped")?
            .map_err(Into::into)
    }
    pub async fn hops(&self) -> Result<u32> {
        match self.request(Body::Describe(DescribeRequest {})).await? {
            Body::Description(description) => Ok(description.hops),
            _ => Err("unexpected topology description".into()),
        }
    }
    pub async fn access(&self, request: AccessRequest) -> Result<AccessResponse> {
        match self.request(Body::Access(request)).await? {
            Body::Result(response) => Ok(response),
            _ => Err("unexpected access response".into()),
        }
    }
}
async fn enqueue<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    pending: &mut HashMap<u64, (Reply, tokio::sync::OwnedSemaphorePermit)>,
    sequence: &mut u64,
    command: Command,
) -> Result<()> {
    // Canceled commands which have not reached the wire consume no request ID.
    if command.reply.is_closed() {
        return Ok(());
    }
    *sequence = sequence
        .checked_add(1)
        .ok_or("WebSocket request IDs exhausted")?;
    pending.insert(*sequence, (command.reply, command.permit));
    socket
        .feed(Message::Binary(
            AccessEnvelope {
                id: *sequence,
                body: Some(command.body),
            }
            .encode_to_vec()
            .into(),
        ))
        .await?;
    Ok(())
}
async fn drive<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    mut commands: mpsc::Receiver<Command>,
) {
    let mut pending = HashMap::with_capacity(IN_FLIGHT);
    let mut sequence = 0;
    let result: Result<()> = async {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { return Ok(()); };
                    enqueue(&mut socket, &mut pending, &mut sequence, command).await?;
                    while pending.len() < IN_FLIGHT {
                        let Ok(command) = commands.try_recv() else { break; };
                        enqueue(&mut socket, &mut pending, &mut sequence, command).await?;
                    }
                    socket.flush().await?;
                }
                message = socket.next() => {
                    match message.ok_or("WebSocket closed")?? {
                        Message::Binary(bytes) => {
                            let envelope = AccessEnvelope::decode(bytes)?;
                            let (reply, _permit) = pending.remove(&envelope.id).ok_or("unknown WebSocket response ID")?;
                            let result = match envelope.body {
                                Some(Body::Error(error)) => Err(error),
                                Some(body) => Ok(body),
                                None => Err("missing response body".into()),
                            };
                            let _ = reply.send(result);
                        }
                        Message::Ping(_) => socket.flush().await?,
                        Message::Pong(_) => (),
                        Message::Close(_) => return Err("WebSocket closed".into()),
                        _ => return Err("unexpected WebSocket frame".into()),
                    }
                }
            }
        }
    }.await;
    let reason = result
        .err()
        .map_or_else(|| "WebSocket client dropped".into(), |e| e.to_string());
    commands.close();
    for (_, (reply, _permit)) in pending {
        let _ = reply.send(Err(reason.clone()));
    }
    while let Ok(command) = commands.try_recv() {
        let _ = command.reply.send(Err(reason.clone()));
    }
}
async fn connection<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    service: Service,
) -> Result<()> {
    let mut work = FuturesUnordered::new();
    loop {
        tokio::select! {
            message = socket.next(), if work.len() < IN_FLIGHT => {
                match message.ok_or("WebSocket closed")?? {
                    Message::Binary(bytes) => {
                        let request = AccessEnvelope::decode(bytes)?;
                        let service = service.clone();
                        work.push(async move {
                            let body = match request.body {
                                Some(Body::Describe(_)) => Body::Description(Description { hops: service.hops }),
                                Some(Body::Access(input)) => match service.access(input).await {
                                    Ok(response) => Body::Result(response),
                                    Err(error) => Body::Error(error.to_string()),
                                },
                                _ => Body::Error("unexpected request body".into()),
                            };
                            AccessEnvelope { id: request.id, body: Some(body) }
                        });
                    }
                    Message::Ping(_) => socket.flush().await?,
                    Message::Pong(_) => (),
                    Message::Close(_) => return Ok(()),
                    _ => return Err("unexpected WebSocket frame".into()),
                }
            }
            response = work.next(), if !work.is_empty() => {
                socket.feed(Message::Binary(response.unwrap().encode_to_vec().into())).await?;
                while let Some(Some(response)) = work.next().now_or_never() {
                    socket.feed(Message::Binary(response.encode_to_vec().into())).await?;
                }
                socket.flush().await?;
            }
        }
    }
}
pub(super) async fn serve(address: &str, service: Service) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    let config_tls = crate::benchmark_tls::server_config()?;
    let sessions = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
    ready("websocket", listener.local_addr()?, service.hops)?;
    loop {
        let permit = sessions.clone().acquire_owned().await?;
        let (socket, _) = listener.accept().await?;
        let service = service.clone();
        let config_tls = config_tls.clone();
        tokio::task::spawn_local(async move {
            let _permit = permit;
            let result: Result<()> = async {
                let socket = crate::benchmark_tls::accept(socket, config_tls).await?;
                let socket = timeout(
                    super::super::DEADLINE,
                    tokio_tungstenite::accept_async_with_config(socket, Some(config())),
                )
                .await??;
                connection(socket, service).await
            }
            .await;
            if let Err(error) = result {
                eprintln!("WebSocket topology closed: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::Role;

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_responses_are_correlated_instead_of_assuming_fifo() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (a, b) = tokio::io::duplex(16384);
                let client = Client::from_socket(
                    WebSocketStream::from_raw_socket(a, Role::Client, Some(config())).await,
                );
                let peer = tokio::task::spawn_local(async move {
                    let mut socket =
                        WebSocketStream::from_raw_socket(b, Role::Server, Some(config())).await;
                    let mut requests = Vec::new();
                    for _ in 0..2 {
                        let Message::Binary(bytes) = socket.next().await.unwrap().unwrap() else {
                            panic!("binary request");
                        };
                        requests.push(AccessEnvelope::decode(bytes).unwrap());
                    }
                    socket.send(Message::Ping(Vec::new().into())).await.unwrap();
                    for request in requests.into_iter().rev() {
                        let Some(Body::Access(input)) = request.body else {
                            panic!("access request");
                        };
                        socket
                            .send(Message::Binary(
                                AccessEnvelope {
                                    id: request.id,
                                    body: Some(Body::Result(AccessResponse {
                                        checksum: input.sequence,
                                        visits: 1,
                                        root: input.root,
                                    })),
                                }
                                .encode_to_vec()
                                .into(),
                            ))
                            .await
                            .unwrap();
                    }
                    // Keep the connection alive until the client releases its owners.
                    while socket.next().await.is_some() {}
                });
                let request = |sequence| AccessRequest {
                    sequence,
                    depth: 1,
                    root: Some(fill(1, sequence)),
                };
                let (a, b) =
                    tokio::try_join!(client.access(request(42)), client.access(request(43)))
                        .unwrap();
                assert_eq!((a.checksum, b.checksum), (42, 43));
                drop(client);
                peer.await.unwrap();
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_response_ids_fail_all_waiters_and_release_capacity() {
        tokio::task::LocalSet::new()
            .run_until(timeout(std::time::Duration::from_secs(5), async {
                let (a, b) = tokio::io::duplex(16384);
                let client = Client::from_socket(
                    WebSocketStream::from_raw_socket(a, Role::Client, Some(config())).await,
                );
                let peer = tokio::task::spawn_local(async move {
                    let mut socket =
                        WebSocketStream::from_raw_socket(b, Role::Server, Some(config())).await;
                    for _ in 0..IN_FLIGHT {
                        assert!(matches!(
                            socket.next().await.unwrap().unwrap(),
                            Message::Binary(_)
                        ));
                    }
                    socket
                        .send(Message::Binary(
                            AccessEnvelope {
                                id: 0,
                                body: Some(Body::Description(Description { hops: 1 })),
                            }
                            .encode_to_vec()
                            .into(),
                        ))
                        .await
                        .unwrap();
                    while socket.next().await.is_some() {}
                });
                // More callers than the window must wait without putting
                // additional requests on the wire. Failure wakes both groups.
                let results =
                    futures::future::join_all((0..IN_FLIGHT * 2).map(|_| client.hops())).await;
                let errors: Vec<_> = results
                    .into_iter()
                    .map(|r| r.unwrap_err().to_string())
                    .collect();
                assert_eq!(
                    errors
                        .iter()
                        .filter(|e| *e == "unknown WebSocket response ID")
                        .count(),
                    IN_FLIGHT
                );
                assert_eq!(
                    errors
                        .iter()
                        .filter(|e| *e == "WebSocket connection stopped")
                        .count(),
                    IN_FLIGHT
                );
                assert_eq!(client.permits.available_permits(), IN_FLIGHT);
                assert!(client.hops().await.is_err());
                peer.await.unwrap();
            }))
            .await
            .unwrap();
    }
}
