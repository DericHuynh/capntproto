use super::{
    application::{expect, request},
    grpc, websocket,
};
use crate::{
    grpc::{operation::Kind, Operation},
    Result,
};
use futures::future::try_join_all;
use serde_json::json;
use std::time::{Duration, Instant};

enum Client {
    Grpc(Box<grpc::Client>),
    Websocket(Box<websocket::Client>),
}
impl Client {
    async fn call(&mut self, req: Operation) -> Result<Operation> {
        let kind = Kind::try_from(req.kind)?;
        let sequence = req.sequence;
        let response = match self {
            Self::Grpc(c) => c.call(req).await?,
            Self::Websocket(c) => c.call(req).await?,
        };
        expect(&response, kind, sequence)?;
        Ok(response)
    }
    async fn send(&mut self, req: Operation) -> Result<()> {
        match self {
            Self::Grpc(c) => c.send(req).await,
            Self::Websocket(c) => c.send(req).await,
        }
    }
    async fn receive(&mut self) -> Result<Operation> {
        match self {
            Self::Grpc(c) => c.receive().await,
            Self::Websocket(c) => c.receive().await,
        }
    }
    async fn open(&mut self, payload: &[u8], sequence: u64, ephemeral: bool) -> Result<u64> {
        let mut req = request(Kind::Open, sequence, payload);
        req.ephemeral = ephemeral;
        let response = self.call(req).await?;
        if response.cell == 0 || response.version != 0 {
            return Err("invalid new cell".into());
        }
        Ok(response.cell)
    }
    async fn read(&mut self, cell: u64, sequence: u64, version: u64, payload: &[u8]) -> Result<()> {
        let mut req = request(Kind::Read, sequence, &[]);
        req.cell = cell;
        let response = self.call(req).await?;
        if response.cell != cell || response.version != version || response.payload != payload {
            return Err("state read mismatch".into());
        }
        Ok(())
    }
    async fn operation(
        &mut self,
        case: &str,
        cell: u64,
        sequence: u64,
        payload: &[u8],
    ) -> Result<()> {
        match case {
            "fanout-32" => match self {
                Self::Grpc(client) => {
                    try_join_all((0..32).map(|i| {
                        let mut rpc = client.rpc.clone();
                        async move {
                            let sequence = sequence * 32 + i;
                            let reply = rpc
                                .echo(request(Kind::Echo, sequence, payload))
                                .await?
                                .into_inner();
                            validate_echo(reply, sequence, payload)
                        }
                    }))
                    .await?;
                }
                Self::Websocket(client) => {
                    for i in 0..32 {
                        client
                            .feed(request(Kind::Echo, sequence * 32 + i, payload))
                            .await?;
                    }
                    client.flush().await?;
                    // This application server replies in arrival order. Every
                    // sequence is checked; duplicate/lost replies cannot pass.
                    for i in 0..32 {
                        validate_echo(client.receive().await?, sequence * 32 + i, payload)?;
                    }
                }
            },
            "dependent-read" => {
                let opened = self.open(payload, sequence, true).await?;
                self.read(opened, sequence, 0, payload).await?;
            }
            "write-read" => {
                let mut req = request(Kind::Write, sequence, payload);
                req.cell = cell;
                req.version = sequence;
                let response = self.call(req).await?;
                if response.cell != cell || response.version != sequence + 1 {
                    return Err("write version mismatch".into());
                }
                self.read(cell, sequence, sequence + 1, payload).await?;
            }
            "callback" => {
                self.send(request(Kind::Callback, sequence, payload))
                    .await?;
                let notify = self.receive().await?;
                expect(&notify, Kind::Notify, sequence)?;
                if notify.payload != payload {
                    return Err("callback payload mismatch".into());
                }
                self.send(request(Kind::Ack, sequence, &[])).await?;
                expect(&self.receive().await?, Kind::Done, sequence)?;
            }
            "pipelined-read" => {
                return Err("promise pipelining is not supported by this protocol".into())
            }
            _ => validate_echo(
                self.call(request(Kind::Echo, sequence, payload)).await?,
                sequence,
                payload,
            )?,
        }
        Ok(())
    }
}
fn validate_echo(response: Operation, sequence: u64, payload: &[u8]) -> Result<()> {
    expect(&response, Kind::Echo, sequence)?;
    if response.payload != payload {
        return Err("echo payload mismatch".into());
    }
    Ok(())
}
pub async fn serve(implementation: &str, address: &str) -> Result<()> {
    match implementation {
        "grpc" => grpc::serve(address).await,
        "websocket" => websocket::serve(address).await,
        _ => Err("unknown comparison protocol".into()),
    }
}
pub async fn measure(implementation: &str, args: &[String]) -> Result<()> {
    if args.len() != 4 {
        return Err("comparison-measure PROTOCOL ADDRESS CASE WARMUP ITERATIONS".into());
    }
    let case = super::cases()
        .into_iter()
        .find(|c| c["name"] == args[1])
        .ok_or("unknown workflow")?;
    if !super::support::supported(implementation, &args[1]) {
        return Err("unsupported workflow/protocol".into());
    }
    let warmup: u64 = args[2].parse()?;
    let iterations: u64 = args[3].parse()?;
    if warmup > 100_000 || iterations == 0 || iterations > 100_000 {
        return Err("workflow budget exceeded".into());
    }
    let payload: Vec<_> = (0..case["payload_bytes"].as_u64().unwrap())
        .map(|i| (i % 251) as u8)
        .collect();
    let deadline = Duration::from_secs(10);
    let mut client = tokio::time::timeout(deadline, async {
        match implementation {
            "grpc" => Ok(Client::Grpc(Box::new(
                grpc::Client::connect(&args[0], args[1] == "callback").await?,
            ))),
            "websocket" => Ok(Client::Websocket(Box::new(
                websocket::Client::connect(&args[0]).await?,
            ))),
            _ => Err::<_, Box<dyn std::error::Error>>("unknown comparison protocol".into()),
        }
    })
    .await??;
    let cell = tokio::time::timeout(deadline, client.open(&payload, 0, false)).await??;
    for sequence in 0..warmup {
        tokio::time::timeout(
            deadline,
            client.operation(&args[1], cell, sequence, &payload),
        )
        .await??;
    }
    let mut samples = Vec::with_capacity(iterations as usize);
    let start = Instant::now();
    for sequence in warmup..warmup + iterations {
        let before = Instant::now();
        tokio::time::timeout(
            deadline,
            client.operation(&args[1], cell, sequence, &payload),
        )
        .await??;
        samples.push(u64::try_from(before.elapsed().as_nanos())?);
    }
    let elapsed = u64::try_from(start.elapsed().as_nanos())?;
    let mut release = request(Kind::Release, warmup + iterations, &[]);
    release.cell = cell;
    tokio::time::timeout(deadline, client.call(release)).await??;
    println!(
        "{}",
        json!({"workflow_version":super::VERSION,"implementation":implementation,"security":crate::transport_metadata(implementation)?,"case":case,"warmup":warmup,"iterations":iterations,"elapsed_ns":elapsed,"latency_ns":samples})
    );
    Ok(())
}
