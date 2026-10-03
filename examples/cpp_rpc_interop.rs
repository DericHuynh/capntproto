//! Real C++ peer interop on a plain ordered stream, independent of Native framing.
use capnp::capability::FromClientHook;
use capntproto_test_support::runtime_test_capnp::harness;
use futures::AsyncReadExt;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    rc::Rc,
    time::Duration,
};
use tokio_util::compat::TokioAsyncReadCompatExt;
struct Peer(Child);
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Callback;
impl harness::Server for Callback {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        if p.get()?.get_value() == 999 {
            return Err(capnp::Error::overloaded("callback failed".into()));
        }
        r.get().set_value(200 + p.get()?.get_value());
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        mut r: harness::BounceResults,
    ) -> capnp::Result<()> {
        r.get().set_cap(p.get()?.get_cap()?);
        Ok(())
    }
    async fn tail(
        self: Rc<Self>,
        p: harness::TailParams,
        r: harness::TailResults,
    ) -> capnp::Result<()> {
        let params = p.get()?;
        let mut request = params.get_cap()?.echo_request();
        request.get().set_value(params.get_value());
        r.hook.tail_call(request.hook).await
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let executable = std::env::args().nth(1).expect("C++ peer executable");
    let mut peer = Peer(
        Command::new(executable)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut port = String::new();
    BufReader::new(peer.0.stdout.take().unwrap())
        .read_line(&mut port)
        .unwrap();
    let port: u16 = port.trim().parse().unwrap();
    tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(Duration::from_secs(10),async {
            let stream = tokio::net::TcpStream::connect(("127.0.0.1",port)).await.unwrap();
            let (read,write) = stream.compat().split();
            let network = capnp_rpc::twoparty::VatNetwork::new(read,write,capnp_rpc::rpc_twoparty_capnp::Side::Client,Default::default());
            let mut system = capnp_rpc::RpcSystem::new(Box::new(network),None);
            system.set_trace_encoder(|error| format!("rust-trace:{}", error.extra));
            let remote: harness::Client = system.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);
            let disconnect = system.get_disconnector();
            let joiner = system.get_joiner();
            let task = tokio::task::spawn_local(system);
            let mut request = remote.echo_request(); request.get().set_value(1);
            assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value(),101);
            let local: harness::Client = capnp_rpc::new_client(Callback);
            let mut request = remote.bounce_request(); request.get().set_cap(local.clone());
            let response = request.send();
            let mut early = response.pipeline.get_cap().echo_request(); early.get().set_value(2);
            assert_eq!(early.send().promise.await.unwrap().get().unwrap().get_value(),202);
            let returned = capnp::capability::get_resolved_cap(response.promise.await.unwrap().get().unwrap().get_cap().unwrap()).await;
            assert_eq!(local.as_client_hook().get_ptr(),returned.as_client_hook().get_ptr());
            let mut request = remote.tail_request(); request.get().set_cap(local.clone()); request.get().set_value(3);
            assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value(),203);
            // C++ makes an ordinary call to Rust, which tail-calls C++ back.
            // C++ therefore consumes Rust's optimized takeFromOtherQuestion.
            let mut request = remote.tail_roundtrip_request(); request.get().set_cap(local.clone()); request.get().set_value(4);
            assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value(),104);
            let mut request = remote.bounce_request(); request.get().set_cap(local.clone());
            let pipeline = request.send_for_pipeline();
            let pipelined = pipeline.get_cap(); drop(pipeline);
            let mut child = pipelined.echo_request(); child.get().set_value(8);
            assert_eq!(child.send().promise.await.unwrap().get().unwrap().get_value(),208);
            drop(pipelined);
            let mut request = remote.tail_roundtrip_request(); request.get().set_cap(local.clone()); request.get().set_value(1000);
            assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value(),105);
            let mut request = remote.tail_roundtrip_request(); request.get().set_cap(local); request.get().set_value(999);
            assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value(),999);
            let mut request = remote.echo_request(); request.get().set_value(1001);
            let error = request.send().promise.await.err().expect("C++ exception");
            assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
            assert!(error.extra.contains("cpp failure"));
            assert!(error.remote_trace().unwrap().contains("cpp-trace:cpp failure"));
            // The C++ dispatcher rejects standard Join. Distinct exports force
            // a wire request; the rejection must preserve both capabilities.
            let request = remote.pending_request();
            let child = request.send().promise.await.unwrap().get().unwrap().get_cap().unwrap();
            let error = joiner.join(vec![remote.clone(), child.clone()]).await.err().expect("C++ Join rejection");
            assert_eq!(error.kind, capnp::ErrorKind::Unimplemented);
            for cap in [remote, child] {
                let mut request = cap.echo_request(); request.get().set_value(6);
                assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value(), 106);
            }
            disconnect.await.unwrap(); task.abort();
            println!("C++ RPC interop: bootstrap, call, callback, pipeline, identity, bidirectional tail calls, pipeline-only calls in both directions, bidirectional exception traces, Join rejection with continued capability use passed");
        }).await.unwrap();
    }).await;
}
