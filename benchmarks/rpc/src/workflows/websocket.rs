use super::application::{finish, notify, State};
use crate::{
    grpc::{operation::Kind, Operation},
    Result,
};
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message},
    WebSocketStream,
};

pub(super) struct Client(WebSocketStream<tokio_rustls::TlsStream<TcpStream>>);
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(super::MAX_MESSAGE_BYTES))
        .max_frame_size(Some(super::MAX_MESSAGE_BYTES))
}
impl Client {
    pub async fn connect(address: &str) -> Result<Self> {
        let socket = crate::benchmark_tls::connect(address).await?;
        let (socket, _) = tokio_tungstenite::client_async_with_config(
            format!("wss://{address}/workflows"),
            socket,
            Some(config()),
        )
        .await?;
        Ok(Self(socket))
    }
    pub async fn feed(&mut self, req: Operation) -> Result<()> {
        self.0
            .feed(Message::Binary(req.encode_to_vec().into()))
            .await?;
        Ok(())
    }
    pub async fn flush(&mut self) -> Result<()> {
        self.0.flush().await?;
        Ok(())
    }
    pub async fn send(&mut self, req: Operation) -> Result<()> {
        self.feed(req).await?;
        self.flush().await
    }
    pub async fn receive(&mut self) -> Result<Operation> {
        match self.0.next().await.ok_or("WebSocket closed")?? {
            Message::Binary(bytes) => Ok(Operation::decode(bytes)?),
            _ => Err("unexpected WebSocket message".into()),
        }
    }
    pub async fn call(&mut self, req: Operation) -> Result<Operation> {
        self.send(req).await?;
        self.receive().await
    }
}
pub(super) async fn serve(address: &str) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    crate::ready(listener.local_addr()?, None, "websocket")?;
    loop {
        let (socket, _) = listener.accept().await?;
        let socket =
            crate::benchmark_tls::accept(socket, crate::benchmark_tls::server_config()?).await?;
        tokio::task::spawn_local(async move {
            let result = async {
                let socket =
                    tokio_tungstenite::accept_async_with_config(socket, Some(config())).await?;
                let mut client = Client(socket);
                let mut state = State::default();
                while let Some(message) = client.0.next().await {
                    let bytes = match message? {
                        Message::Binary(bytes) => bytes,
                        Message::Close(_) => return Ok::<_, Box<dyn std::error::Error>>(()),
                        _ => return Err("unexpected WebSocket frame".into()),
                    };
                    let req = Operation::decode(bytes)?;
                    let reply = if req.kind == Kind::Callback as i32 {
                        let sequence = req.sequence;
                        client.send(notify(req)?).await?;
                        finish(client.receive().await?, sequence)?
                    } else {
                        state.apply(req)?
                    };
                    client.send(reply).await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                eprintln!("WebSocket workflow closed: {error}");
            }
        });
    }
}
