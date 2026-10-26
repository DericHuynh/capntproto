use super::*;
use crate::grpc::{
    access_service_client::AccessServiceClient,
    access_service_server::{AccessService, AccessServiceServer},
    DescribeRequest, Description,
};
use tokio::net::TcpListener;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub(super) struct Client(AccessServiceClient<tonic::transport::Channel>);
impl Client {
    pub async fn connect(address: &str) -> Result<Self> {
        Ok(Self(AccessServiceClient::new(
            crate::grpc_channel(address).await?,
        )))
    }
    pub async fn hops(&self) -> Result<u32> {
        Ok(self
            .0
            .clone()
            .describe(DescribeRequest {})
            .await?
            .into_inner()
            .hops)
    }
    pub async fn access(&self, request: AccessRequest) -> Result<AccessResponse> {
        Ok(self.0.clone().access(request).await?.into_inner())
    }
}
#[tonic::async_trait]
impl AccessService for Service {
    async fn describe(
        &self,
        _: Request<DescribeRequest>,
    ) -> std::result::Result<Response<Description>, Status> {
        Ok(Response::new(Description { hops: self.hops }))
    }
    async fn access(
        &self,
        request: Request<AccessRequest>,
    ) -> std::result::Result<Response<AccessResponse>, Status> {
        self.access(request.into_inner())
            .await
            .map(Response::new)
            .map_err(|error| Status::invalid_argument(error.to_string()))
    }
}
pub(super) async fn serve(address: &str, service: Service) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    ready("grpc", listener.local_addr()?, service.hops)?;
    tonic::transport::Server::builder()
        .add_service(AccessServiceServer::new(service))
        .serve_with_incoming(crate::grpc_incoming(listener)?)
        .await?;
    Ok(())
}
