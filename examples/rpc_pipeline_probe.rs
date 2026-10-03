//! Finite protocol experiments with real RPC framing and a simulated clock.
//! This is not a TCP/QUIC throughput benchmark. See docs/wiki/RPC-Research.md.
use capnp::capability::{get_resolved_cap, RemotePromise};
use capnp_rpc::{
    rpc_capnp::{message, message_target},
    rpc_twoparty_capnp::Side,
    PipelineBuilder, RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::AsyncReadExt as _;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf},
    time::Instant,
};
use tokio_util::compat::TokioAsyncReadCompatExt;

#[derive(Clone, Default)]
struct Wire {
    messages: BTreeMap<&'static str, u64>,
    bytes: u64,
    promised_targets: u64,
    pipeline_only: u64,
    no_pipeline: u64,
}
impl Wire {
    fn observe(&mut self, m: message::Reader<'_>, bytes: usize) -> capnp::Result<()> {
        let kind = match m.which()? {
            message::Call(call) => {
                let call = call?;
                self.promised_targets += u64::from(matches!(
                    call.get_target()?.which()?,
                    message_target::PromisedAnswer(_)
                ));
                self.pipeline_only += u64::from(call.get_only_promise_pipeline());
                self.no_pipeline += u64::from(call.get_no_promise_pipelining());
                "call"
            }
            message::Return(_) => "return",
            message::Finish(_) => "finish",
            message::Release(_) => "release",
            message::Resolve(_) => "resolve",
            message::Bootstrap(_) => "bootstrap",
            message::Disembargo(_) => "disembargo",
            message::Abort(_) => panic!("unexpected RPC abort"),
            _ => panic!("unexpected protocol message in bilateral probe"),
        };
        *self.messages.entry(kind).or_default() += 1;
        self.bytes += bytes as u64;
        Ok(())
    }
    fn count(&self, name: &str) -> u64 {
        self.messages.get(name).copied().unwrap_or(0)
    }
    fn delta(&self, before: &Self) -> Value {
        let messages: BTreeMap<_, _> = self
            .messages
            .iter()
            .map(|(k, v)| (*k, v - before.count(k)))
            .collect();
        json!({"messages": messages, "framed_bytes": self.bytes - before.bytes,
            "promised_targets": self.promised_targets - before.promised_targets,
            "pipeline_only_calls": self.pipeline_only - before.pipeline_only,
            "no_pipeline_calls": self.no_pipeline - before.no_pipeline})
    }
}

// Schedule each frame relative to its own arrival, allowing propagation delays
// to overlap. Sleeping after every frame in one reader loop would accidentally
// serialize the pipeline. The bounded relay is below capacity in every case.
async fn relay(
    read: ReadHalf<DuplexStream>,
    mut write: WriteHalf<DuplexStream>,
    delay: Duration,
    wire: Rc<RefCell<Wire>>,
) -> capnp::Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(Instant, Vec<u8>)>(256);
    let input = async move {
        let mut read = read.compat();
        while let Some(frame) =
            capnp_futures::serialize::try_read_message(&mut read, Default::default()).await?
        {
            let bytes = capnp::serialize::write_message_segments_to_words(frame.get_segments());
            wire.borrow_mut().observe(frame.get_root()?, bytes.len())?;
            tx.try_send((Instant::now() + delay, bytes))
                .expect("finite relay capacity exceeded or writer failed");
        }
        Ok::<_, capnp::Error>(())
    };
    let output = async move {
        while let Some((when, bytes)) = rx.recv().await {
            if when > Instant::now() {
                tokio::time::sleep_until(when).await;
            }
            write.write_all(&bytes).await?;
        }
        write.shutdown().await?;
        Ok::<_, capnp::Error>(())
    };
    futures::try_join!(input, output)?;
    Ok(())
}

#[derive(Default)]
struct State {
    calls: Cell<usize>,
    parent_completed: Cell<bool>,
}
struct Service {
    state: Rc<State>,
    early: bool,
}
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.state.calls.set(self.state.calls.get() + 1);
        results.get().set_value(params.get()?.get_value());
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        mut results: harness::BounceResults,
    ) -> capnp::Result<()> {
        self.state.calls.set(self.state.calls.get() + 1);
        results.get().set_cap(params.get()?.get_cap()?);
        Ok(())
    }
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        self.state.calls.set(self.state.calls.get() + 1);
        let target: harness::Client = capnp_rpc::new_client_from_rc(self.clone());
        if self.early {
            let mut pipeline = PipelineBuilder::<harness::pending_results::Owned>::new();
            pipeline.get().set_cap(target.clone());
            results.set_pipeline_from(pipeline.build())?;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        results.get().set_cap(target);
        self.state.parent_completed.set(true);
        Ok(())
    }
}

struct Fixture {
    client: harness::Client,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
    wire: [Rc<RefCell<Wire>>; 2],
    state: Rc<State>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Fixture {
    async fn new(rtt_ms: u64, early: bool) -> capnp::Result<Self> {
        let state = Rc::new(State::default());
        let bootstrap: harness::Client = capnp_rpc::new_client(Service {
            state: state.clone(),
            early,
        });
        let (client, left) = tokio::io::duplex(64 * 1024);
        let (right, server) = tokio::io::duplex(64 * 1024);
        let (lr, lw) = tokio::io::split(left);
        let (rr, rw) = tokio::io::split(right);
        let wire = [
            Rc::new(RefCell::new(Wire::default())),
            Rc::new(RefCell::new(Wire::default())),
        ];
        let delay = Duration::from_millis(rtt_ms / 2);
        let (cr, cw) = client.compat().split();
        let (sr, sw) = server.compat().split();
        let mut client_system = RpcSystem::new(
            Box::new(capnp_rpc::twoparty::VatNetwork::new(
                cr,
                cw,
                Side::Client,
                Default::default(),
            )),
            None,
        );
        let server_system = RpcSystem::new(
            Box::new(capnp_rpc::twoparty::VatNetwork::new(
                sr,
                sw,
                Side::Server,
                Default::default(),
            )),
            Some(bootstrap.client),
        );
        let client = client_system.bootstrap(Side::Server);
        let tasks = vec![
            tokio::task::spawn_local(relay(lr, rw, delay, wire[0].clone())),
            tokio::task::spawn_local(relay(rr, lw, delay, wire[1].clone())),
            tokio::task::spawn_local(client_system),
            tokio::task::spawn_local(server_system),
        ];
        let mut fixture = Self {
            client,
            tasks,
            wire,
            state,
        };
        fixture.client = get_resolved_cap(fixture.client.clone()).await;
        Ok(fixture)
    }
    async fn close(mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks.drain(..) {
            match task.await {
                Err(e) if e.is_cancelled() => (),
                other => panic!("driver stopped before explicit probe cleanup: {other:?}"),
            }
        }
    }
}

async fn echo(client: &harness::Client) -> capnp::Result<()> {
    let mut call = client.echo_request();
    call.get().set_value(42);
    assert_eq!(call.send().promise.await?.get()?.get_value(), 42);
    Ok(())
}

async fn measure(mode: &str, rtt_ms: u64) -> capnp::Result<Value> {
    let fixture = Fixture::new(rtt_ms, mode == "early-publication").await?;
    let before = [
        fixture.wire[0].borrow().clone(),
        fixture.wire[1].borrow().clone(),
    ];
    let started = Instant::now();
    let child_at;
    let parent_at;
    let mut child_before_parent = false;
    let depth;
    if mode.ends_with("publication") {
        depth = 1;
        let parent = fixture.client.pending_request().send();
        echo(&parent.pipeline.get_cap()).await?;
        child_at = started.elapsed();
        child_before_parent = !fixture.state.parent_completed.get();
        parent.promise.await?;
        parent_at = started.elapsed();
        assert_eq!(child_before_parent, mode == "early-publication");
        assert_eq!(fixture.state.calls.get(), 2);
    } else {
        depth = 4;
        let mut current = fixture.client.clone();
        let mut parents: Vec<RemotePromise<harness::bounce_results::Owned>> = Vec::new();
        let mut responses = Vec::new();
        let mut pipelines = Vec::new();
        for _ in 0..depth {
            let mut call = current.bounce_request();
            call.get().set_cap(fixture.client.clone());
            match mode {
                "sequential" => {
                    let response = call.send().promise.await?;
                    current = response.get()?.get_cap()?;
                    responses.push(response);
                }
                "pipelined" => {
                    let parent = call.send();
                    current = parent.pipeline.get_cap();
                    parents.push(parent);
                }
                "pipeline-only" => {
                    let pipeline = call.send_for_pipeline();
                    current = pipeline.get_cap();
                    pipelines.push(pipeline);
                }
                _ => panic!("unknown scenario"),
            }
        }
        echo(&current).await?;
        child_at = started.elapsed();
        for parent in parents {
            parent.promise.await?;
        }
        parent_at = started.elapsed();
        assert_eq!(fixture.state.calls.get(), depth + 1);
        // Retain pipeline-only parents through terminal success; they are not
        // fire-and-forget calls. Dropping them now begins normal Finish cleanup.
        drop((pipelines, responses));
    }
    let forward = fixture.wire[0].borrow().delta(&before[0]);
    let reverse = fixture.wire[1].borrow().delta(&before[1]);
    assert_eq!(forward["messages"]["call"], depth + 1);
    assert_eq!(
        reverse["messages"]["return"],
        if mode == "pipeline-only" {
            1
        } else {
            depth + 1
        }
    );
    assert_eq!(
        forward["pipeline_only_calls"],
        if mode == "pipeline-only" { depth } else { 0 }
    );
    assert_eq!(forward["no_pipeline_calls"], 1); // Generated scalar echo hint.
    if mode != "sequential" {
        assert_eq!(forward["promised_targets"], depth);
    }
    let result = json!({"mode": mode, "configured_rtt_ms": rtt_ms, "depth": depth,
        "terminal_reply_virtual_us": child_at.as_micros() as u64,
        "all_observed_replies_virtual_us": parent_at.as_micros() as u64,
        "child_before_parent_completion": child_before_parent,
        "client_to_server": forward, "server_to_client": reverse, "validated": true});
    fixture.close().await;
    Ok(result)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()?;
    let results = runtime.block_on(tokio::task::LocalSet::new().run_until(async {
        let mut rows = Vec::new();
        for rtt in [0, 2, 20] {
            for mode in [
                "sequential",
                "pipelined",
                "pipeline-only",
                "late-publication",
                "early-publication",
            ] {
                rows.push(
                    tokio::time::timeout(Duration::from_secs(5), measure(mode, rtt))
                        .await
                        .expect("probe stalled")?,
                );
            }
        }
        Ok::<_, capnp::Error>(rows)
    }))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "clock": "Tokio paused virtual clock; microseconds are simulated, not CPU or network measurements",
            "transport": "two in-memory byte streams with bounded per-frame delayed relays; real bilateral RPC framing",
            "wire_scope": "after bootstrap resolution through terminal/observed parent replies, before final cleanup; framing included, encryption absent",
            "scenarios": results,
        }))?
    );
    Ok(())
}
