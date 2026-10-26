//! Idiomatic HTTP/2 and multiplexed WebSocket services for the shared topology.
use crate::{
    grpc::{AccessNode, AccessRequest, AccessResponse},
    Result,
};
use futures::future::try_join_all;
use serde_json::{json, Value};
use std::time::Instant;
use tokio::time::timeout;

mod grpc;
mod websocket;

#[derive(Clone)]
enum Client {
    Grpc(grpc::Client),
    Websocket(websocket::Client),
}
impl Client {
    async fn connect(protocol: &str, address: &str) -> Result<Self> {
        match protocol {
            "grpc" => Ok(Self::Grpc(grpc::Client::connect(address).await?)),
            "websocket" => Ok(Self::Websocket(websocket::Client::connect(address).await?)),
            _ => Err("unknown topology protocol".into()),
        }
    }
    async fn hops(&self) -> Result<u32> {
        match self {
            Self::Grpc(client) => client.hops().await,
            Self::Websocket(client) => client.hops().await,
        }
    }
    async fn access(&self, request: AccessRequest) -> Result<AccessResponse> {
        match self {
            Self::Grpc(client) => client.access(request).await,
            Self::Websocket(client) => client.access(request).await,
        }
    }
}

#[derive(Clone)]
struct Service {
    next: Option<Client>,
    hops: u32,
}
impl Service {
    async fn new(protocol: &str, next: Option<&String>) -> Result<Self> {
        let next = match next {
            Some(address) => {
                Some(timeout(super::DEADLINE, Client::connect(protocol, address)).await??)
            }
            None => None,
        };
        let hops = if let Some(next) = &next {
            let hops = timeout(super::DEADLINE, next.hops()).await??;
            if !(1..8).contains(&hops) {
                return Err("service chain exceeds eight hops".into());
            }
            hops + 1
        } else {
            1
        };
        Ok(Self { next, hops })
    }
    async fn access(&self, request: AccessRequest) -> Result<AccessResponse> {
        let sum = traverse(request.root.as_ref(), request.depth, request.sequence)?;
        let depth = request.depth;
        if let Some(next) = &self.next {
            let mut response = next.access(request).await?;
            response.checksum = response.checksum.wrapping_add(sum);
            response.visits += depth;
            Ok(response)
        } else {
            Ok(AccessResponse {
                checksum: sum,
                visits: depth,
                root: request.root,
            })
        }
    }
}

fn fill(depth: u32, sequence: u64) -> AccessNode {
    let mut next = None;
    for level in (0..depth).rev() {
        next = Some(Box::new(AccessNode {
            value: sequence + u64::from(level),
            payload: super::PAYLOAD.to_vec(),
            next,
        }));
    }
    *next.expect("nonempty checked topology")
}
fn traverse(mut node: Option<&AccessNode>, depth: u32, sequence: u64) -> Result<u64> {
    if !(1..=32).contains(&depth) {
        return Err("invalid depth".into());
    }
    let mut sum = 0u64;
    for level in 0..depth {
        let current = node.ok_or("missing nested node")?;
        let value = sequence + u64::from(level);
        if current.value != value
            || current.payload != super::PAYLOAD
            || current.next.is_some() != (level + 1 < depth)
        {
            return Err("nested access mismatch".into());
        }
        sum = sum.wrapping_add(value);
        for byte in &current.payload {
            sum = sum.wrapping_add(u64::from(*byte));
        }
        node = current.next.as_deref();
    }
    Ok(sum)
}
fn ready(protocol: &str, address: std::net::SocketAddr, hops: u32) -> Result<()> {
    use std::io::Write;
    println!(
        "{}",
        json!({"address":address.to_string(),"hops":hops,
        "implementation":protocol,"security":crate::transport_metadata(protocol)?})
    );
    std::io::stdout().flush()?;
    Ok(())
}
pub async fn serve(protocol: &str, args: &[String]) -> Result<()> {
    if !(1..=2).contains(&args.len()) {
        return Err("comparison-topology-server PROTOCOL ADDRESS [NEXT_ADDRESS]".into());
    }
    let service = Service::new(protocol, args.get(1)).await?;
    match protocol {
        "grpc" => grpc::serve(&args[0], service).await,
        "websocket" => websocket::serve(&args[0], service).await,
        _ => Err("unknown topology protocol".into()),
    }
}
pub async fn measure(protocol: &str, args: &[String]) -> Result<()> {
    if args.len() != 4 {
        return Err("comparison-topology-measure PROTOCOL ADDRESS CASE WARMUP ITERATIONS".into());
    }
    let cases: Vec<Value> = serde_json::from_str(include_str!("../../topologies.json"))?;
    let case = cases
        .into_iter()
        .find(|c| c["name"] == args[1])
        .ok_or("unknown topology")?;
    let depth = case["depth"].as_u64().unwrap() as u32;
    let hops = case["hops"].as_u64().unwrap() as u32;
    let width = case["width"].as_u64().unwrap();
    let connections = case["connections"].as_u64().unwrap() as usize;
    let warmup: u64 = args[2].parse()?;
    let iterations: u64 = args[3].parse()?;
    if warmup > 100_000 || !(1..=100_000).contains(&iterations) {
        return Err("topology budget exceeded".into());
    }
    let mut clients = Vec::with_capacity(connections);
    for _ in 0..connections {
        let client = timeout(super::DEADLINE, Client::connect(protocol, &args[0])).await??;
        if timeout(super::DEADLINE, client.hops()).await?? != hops {
            return Err("wrong service topology".into());
        }
        clients.push(client);
    }
    let operation = |sequence| {
        try_join_all((0..width).map(|lane| {
            let client = &clients[lane as usize % connections];
            async move {
                let sequence = sequence * width + lane;
                let reply = client
                    .access(AccessRequest {
                        sequence,
                        depth,
                        root: Some(fill(depth, sequence)),
                    })
                    .await?;
                let sum = traverse(reply.root.as_ref(), depth, sequence)?;
                if reply.checksum != sum.wrapping_mul(u64::from(hops))
                    || reply.visits != depth * hops
                {
                    return Err("service traversal count/checksum mismatch".into());
                }
                Ok::<_, Box<dyn std::error::Error>>(())
            }
        }))
    };
    for sequence in 0..warmup {
        timeout(super::DEADLINE, operation(sequence)).await??;
    }
    let mut samples = Vec::with_capacity(iterations as usize);
    let start = Instant::now();
    for sequence in warmup..warmup + iterations {
        let before = Instant::now();
        timeout(super::DEADLINE, operation(sequence)).await??;
        samples.push(u64::try_from(before.elapsed().as_nanos())?);
    }
    println!(
        "{}",
        json!({"topology_version":1,"implementation":protocol,
        "security":crate::transport_metadata(protocol)?,"case":case,"warmup":warmup,
        "iterations":iterations,"elapsed_ns":u64::try_from(start.elapsed().as_nanos())?,"latency_ns":samples})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_contract_matches_capability_fixture_and_rejects_corruption() {
        let mut root = fill(32, 42);
        assert_eq!(
            traverse(Some(&root), 32, 42).unwrap(),
            (42..74).sum::<u64>() + 32 * (0..32).sum::<u64>()
        );
        for depth in [0, 31, 33] {
            assert!(traverse(Some(&root), depth, 42).is_err());
        }
        assert!(traverse(None, 32, 42).is_err());
        assert!(traverse(Some(&root), 32, 43).is_err());
        root.next.as_mut().unwrap().payload[0] = 99;
        assert!(traverse(Some(&root), 32, 42).is_err());
    }
}
