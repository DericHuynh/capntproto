//! Nested access through independent TCP services, with bounded concurrent batches.
use crate::{
    echo_capnp::{access_node, access_service},
    Result,
};
use capnp::capability::Promise;
use futures::future::try_join_all;
use serde_json::{json, Value};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, time::timeout};

const DEADLINE: Duration = Duration::from_secs(10);
const PAYLOAD: [u8; 32] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31,
];

fn traverse(mut node: access_node::Reader<'_>, depth: u16, sequence: u64) -> capnp::Result<u64> {
    if !(1..=32).contains(&depth) {
        return Err(capnp::Error::failed("invalid depth".into()));
    }
    let mut sum = 0u64;
    for level in 0..depth {
        let value = sequence + u64::from(level);
        let payload = node.get_payload()?;
        if node.get_value() != value || payload != PAYLOAD || node.has_next() != (level + 1 < depth)
        {
            return Err(capnp::Error::failed("nested access mismatch".into()));
        }
        sum = sum.wrapping_add(value);
        for byte in payload {
            sum = sum.wrapping_add(u64::from(*byte));
        }
        if level + 1 < depth {
            node = node.get_next()?;
        }
    }
    Ok(sum)
}

fn fill(mut node: access_node::Builder<'_>, depth: u16, sequence: u64) {
    for level in 0..depth {
        node.set_value(sequence + u64::from(level));
        node.set_payload(&PAYLOAD);
        if level + 1 < depth {
            node = node.init_next();
        }
    }
}

struct Service {
    next: Option<access_service::Client>,
    hops: u16,
}
impl access_service::Server for Service {
    fn describe(
        self: Rc<Self>,
        context: access_service::DescribeContext<'_>,
    ) -> Promise<(), capnp::Error> {
        context.complete(|_, mut results| {
            results.set_hops(self.hops);
            Ok(())
        })
    }

    fn access(
        self: Rc<Self>,
        context: access_service::AccessContext<'_>,
    ) -> Promise<(), capnp::Error> {
        if self.next.is_none() {
            return context.complete(|params, mut results| {
                let root = params.get_root()?;
                let sum = traverse(root, params.get_depth(), params.get_sequence())?;
                results.set_checksum(sum);
                results.set_visits(u32::from(params.get_depth()));
                results.set_root(root)?;
                Ok(())
            });
        }
        let (params, mut results) = context.into_owned();
        Promise::from_future(async move {
            let params = params.get()?;
            let root = params.get_root()?;
            let sum = traverse(root, params.get_depth(), params.get_sequence())?;
            let mut request = self.next.as_ref().unwrap().access_request();
            request.get().set_sequence(params.get_sequence());
            request.get().set_depth(params.get_depth());
            request.get().set_root(root)?;
            let response = request.send().promise.await?;
            let response = response.get()?;
            let mut results = results.get();
            results.set_checksum(sum.wrapping_add(response.get_checksum()));
            results.set_visits(u32::from(params.get_depth()) + response.get_visits());
            results.set_root(response.get_root()?)?;
            Ok(())
        })
    }
}

async fn connect(address: &str) -> Result<access_service::Client> {
    let mut driver = capntproto::rpc::connect(address, None, Default::default()).await?;
    let client = driver.bootstrap();
    tokio::task::spawn_local(driver);
    Ok(client)
}

pub async fn serve(args: &[String]) -> Result<()> {
    if !(1..=2).contains(&args.len()) {
        return Err("topology-server ADDRESS [NEXT_ADDRESS]".into());
    }
    let next = if let Some(address) = args.get(1) {
        Some(timeout(DEADLINE, connect(address)).await??)
    } else {
        None
    };
    let hops = if let Some(next) = &next {
        let response = timeout(DEADLINE, next.describe_request().send().promise).await??;
        let downstream = response.get()?.get_hops();
        if !(1..8).contains(&downstream) {
            return Err("service chain exceeds eight hops".into());
        }
        downstream + 1
    } else {
        1
    };
    let service: access_service::Client = capnp_rpc::new_client(Service { next, hops });
    let listener = TcpListener::bind(&args[0]).await?;
    println!(
        "{}",
        json!({"address":listener.local_addr()?.to_string(),"hops":hops})
    );
    use std::io::Write;
    std::io::stdout().flush()?;
    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let driver = capntproto::rpc::tcp::client(
            socket,
            Some(service.client.clone()),
            capnp_rpc::rpc_twoparty_capnp::Side::Server,
            Default::default(),
        );
        tokio::task::spawn_local(async move {
            let _ = driver.await;
        });
    }
}

async fn access(
    client: &access_service::Client,
    depth: u16,
    hops: u16,
    sequence: u64,
) -> Result<()> {
    let mut request = client.access_request();
    request.get().set_sequence(sequence);
    request.get().set_depth(depth);
    fill(request.get().init_root(), depth, sequence);
    let response = request.send().promise.await?;
    let response = response.get()?;
    let sum = traverse(response.get_root()?, depth, sequence)?;
    if response.get_checksum() != sum.wrapping_mul(u64::from(hops))
        || response.get_visits() != u32::from(depth) * u32::from(hops)
    {
        return Err("service traversal count/checksum mismatch".into());
    }
    Ok(())
}

pub async fn measure(args: &[String]) -> Result<()> {
    if args.len() != 4 {
        return Err("topology-measure ADDRESS CASE WARMUP ITERATIONS".into());
    }
    let cases: Vec<Value> = serde_json::from_str(include_str!("../topologies.json"))?;
    let case = cases
        .into_iter()
        .find(|c| c["name"] == args[1])
        .ok_or("unknown topology")?;
    let depth = case["depth"].as_u64().unwrap() as u16;
    let hops = case["hops"].as_u64().unwrap() as u16;
    let width = case["width"].as_u64().unwrap();
    let connections = case["connections"].as_u64().unwrap() as usize;
    let warmup: u64 = args[2].parse()?;
    let iterations: u64 = args[3].parse()?;
    if warmup > 100_000 || !(1..=100_000).contains(&iterations) {
        return Err("topology budget exceeded".into());
    }
    let mut clients = Vec::with_capacity(connections);
    for _ in 0..connections {
        let client = timeout(DEADLINE, connect(&args[0])).await??;
        let response = timeout(DEADLINE, client.describe_request().send().promise).await??;
        if response.get()?.get_hops() != hops {
            return Err("wrong service topology".into());
        }
        clients.push(client);
    }
    let operation = |sequence| {
        try_join_all((0..width).map(|lane| {
            access(
                &clients[lane as usize % connections],
                depth,
                hops,
                sequence * width + lane,
            )
        }))
    };
    for sequence in 0..warmup {
        timeout(DEADLINE, operation(sequence)).await??;
    }
    let mut samples = Vec::with_capacity(iterations as usize);
    let start = Instant::now();
    for sequence in warmup..warmup + iterations {
        let before = Instant::now();
        timeout(DEADLINE, operation(sequence)).await??;
        samples.push(u64::try_from(before.elapsed().as_nanos())?);
    }
    println!(
        "{}",
        json!({"topology_version":1,"implementation":"legacy","case":case,"warmup":warmup,"iterations":iterations,"elapsed_ns":u64::try_from(start.elapsed().as_nanos())?,"latency_ns":samples})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_contract_rejects_corruption_and_wrong_depth() {
        let mut message = capnp::message::Builder::new_default();
        fill(message.init_root(), 32, 42);
        let root: access_node::Reader<'_> = message.get_root_as_reader().unwrap();
        assert_eq!(
            traverse(root, 32, 42).unwrap(),
            (42..74).sum::<u64>() + 32 * (0..32).sum::<u64>()
        );
        assert!(traverse(root, 31, 42).is_err());
        assert!(traverse(root, 32, 43).is_err());
        let mut root: access_node::Builder<'_> = message.get_root().unwrap();
        root.reborrow().get_next().unwrap().set_payload(&[1]);
        assert!(traverse(message.get_root_as_reader().unwrap(), 32, 42).is_err());
    }
}
