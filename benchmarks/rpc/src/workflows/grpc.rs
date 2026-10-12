use super::application::{finish, notify, State};
use crate::{
    grpc::{
        operation::Kind,
        workflows_client::WorkflowsClient,
        workflows_server::{Workflows, WorkflowsServer},
        Operation,
    },
    Result,
};
use futures::StreamExt;
use std::sync::Mutex;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Request, Response, Status, Streaming};

#[derive(Default)]
struct Service(Mutex<State>);
impl Service {
    fn apply(
        &self,
        req: Request<Operation>,
        kind: Kind,
    ) -> std::result::Result<Response<Operation>, Status> {
        let req = req.into_inner();
        if req.kind != kind as i32 {
            return Err(Status::invalid_argument("method/kind mismatch"));
        }
        self.0
            .lock()
            .map_err(|_| Status::internal("state poisoned"))?
            .apply(req)
            .map(Response::new)
    }
}
#[tonic::async_trait]
impl Workflows for Service {
    async fn echo(
        &self,
        req: Request<Operation>,
    ) -> std::result::Result<Response<Operation>, Status> {
        self.apply(req, Kind::Echo)
    }
    async fn open(
        &self,
        req: Request<Operation>,
    ) -> std::result::Result<Response<Operation>, Status> {
        self.apply(req, Kind::Open)
    }
    async fn read(
        &self,
        req: Request<Operation>,
    ) -> std::result::Result<Response<Operation>, Status> {
        self.apply(req, Kind::Read)
    }
    async fn write(
        &self,
        req: Request<Operation>,
    ) -> std::result::Result<Response<Operation>, Status> {
        self.apply(req, Kind::Write)
    }
    async fn release(
        &self,
        req: Request<Operation>,
    ) -> std::result::Result<Response<Operation>, Status> {
        self.apply(req, Kind::Release)
    }
    type CallbackStream = ReceiverStream<std::result::Result<Operation, Status>>;
    async fn callback(
        &self,
        req: Request<Streaming<Operation>>,
    ) -> std::result::Result<Response<Self::CallbackStream>, Status> {
        let mut incoming = req.into_inner();
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            let result = async {
                while let Some(req) = incoming.message().await? {
                    let sequence = req.sequence;
                    tx.send(Ok(notify(req)?))
                        .await
                        .map_err(|_| Status::cancelled("callback closed"))?;
                    let ack = incoming
                        .message()
                        .await?
                        .ok_or_else(|| Status::cancelled("missing acknowledgement"))?;
                    tx.send(Ok(finish(ack, sequence)?))
                        .await
                        .map_err(|_| Status::cancelled("callback closed"))?;
                }
                Ok::<_, Status>(())
            }
            .await;
            if let Err(error) = result {
                let _ = tx.send(Err(error)).await;
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
pub(super) async fn serve(address: &str) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    crate::ready(listener.local_addr()?, None)?;
    let incoming = TcpListenerStream::new(listener).map(|socket| {
        socket.and_then(|socket| {
            socket.set_nodelay(true)?;
            Ok(socket)
        })
    });
    tonic::transport::Server::builder()
        .add_service(
            WorkflowsServer::new(Service::default())
                .max_decoding_message_size(super::MAX_MESSAGE_BYTES)
                .max_encoding_message_size(super::MAX_MESSAGE_BYTES),
        )
        .serve_with_incoming(incoming)
        .await?;
    Ok(())
}
pub(super) struct Client {
    pub rpc: WorkflowsClient<tonic::transport::Channel>,
    tx: Option<mpsc::Sender<Operation>>,
    rx: Option<Streaming<Operation>>,
}
impl Client {
    pub async fn connect(address: &str, callback: bool) -> Result<Self> {
        let mut rpc = WorkflowsClient::connect(format!("http://{address}"))
            .await?
            .max_decoding_message_size(super::MAX_MESSAGE_BYTES)
            .max_encoding_message_size(super::MAX_MESSAGE_BYTES);
        let (tx, rx) = if callback {
            let (tx, rx) = mpsc::channel(1);
            let rx = rpc.callback(ReceiverStream::new(rx)).await?.into_inner();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        Ok(Self { rpc, tx, rx })
    }
    pub async fn call(&mut self, req: Operation) -> Result<Operation> {
        Ok(match Kind::try_from(req.kind)? {
            Kind::Echo => self.rpc.echo(req).await?,
            Kind::Open => self.rpc.open(req).await?,
            Kind::Read => self.rpc.read(req).await?,
            Kind::Write => self.rpc.write(req).await?,
            Kind::Release => self.rpc.release(req).await?,
            _ => return Err("unsupported unary method".into()),
        }
        .into_inner())
    }
    pub async fn send(&self, req: Operation) -> Result<()> {
        self.tx
            .as_ref()
            .ok_or("callback stream not open")?
            .send(req)
            .await?;
        Ok(())
    }
    pub async fn receive(&mut self) -> Result<Operation> {
        self.rx
            .as_mut()
            .ok_or("callback stream not open")?
            .message()
            .await?
            .ok_or_else(|| "callback stream closed".into())
    }
}
